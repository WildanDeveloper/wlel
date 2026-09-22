//! `wlel lsp` — Language Server Protocol v3.17 over stdio, hand-rolled with
//! zero dependencies (the whole compiler is dependency-free and stays that
//! way). Features (v1): publish diagnostics (syntax + type + warnings),
//! hover (types/signatures from the checker's annotations), go-to-definition
//! (locals, functions, methods, structs, enums, variants, fields, imports)
//! and completion (locals + top-level symbols + std + keywords).
//!
//! The heavy lifting reuses the normal pipeline: lex → parse (error
//! recovery) → import merge (same semantics as `project::load_program`) →
//! `use std` splice → checker. A pre-check clone of the program ("index")
//! keeps generic templates and und desugared `impl` methods available for
//! lookup, while the post-check program carries concrete types.

use crate::ast::*;
use crate::checker::Checker;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::project::load_program;
use crate::span::{Pos, Span};
use crate::stdsrc::{splice_std, STD_FILE};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// minimal JSON: parser + writer (numbers keep i64 precision for ids/positions)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

struct JsonParser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Json {
    pub fn parse(s: &str) -> Result<Json, String> {
        let mut p = JsonParser { b: s.as_bytes(), i: 0 };
        p.ws();
        let v = p.value()?;
        p.ws();
        if p.i != p.b.len() {
            return Err(format!("trailing data at byte {}", p.i));
        }
        Ok(v)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// nested lookup: `msg.ptr(&["params", "textDocument", "uri"])`
    pub fn ptr(&self, path: &[&str]) -> Option<&Json> {
        let mut cur = self;
        for k in path {
            cur = cur.get(k)?;
        }
        Some(cur)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(i) => Some(*i),
            Json::Num(f) if f.fract() == 0.0 => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(v) => Some(v),
            _ => None,
        }
    }
}

impl<'a> JsonParser<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len()
            && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.peek() {
            None => Err("unexpected end of input".into()),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'n') => self.lit("null", Json::Null),
            Some(_) => self.number(),
        }
    }

    fn lit(&mut self, word: &str, v: Json) -> Result<Json, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("invalid literal at byte {}", self.i))
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.i += 1; // {
        let mut out: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(out));
        }
        loop {
            self.ws();
            let k = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(format!("expected ':' at byte {}", self.i));
            }
            self.i += 1;
            self.ws();
            let v = self.value()?;
            out.push((k, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Obj(out));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.i)),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.i += 1; // [
        let mut out: Vec<Json> = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(out));
        }
        loop {
            self.ws();
            out.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Arr(out));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.i)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.peek() != Some(b'"') {
            return Err(format!("expected string at byte {}", self.i));
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated string".into()),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some(b'"') => out.push('"'),
                        Some(b'\\') => out.push('\\'),
                        Some(b'/') => out.push('/'),
                        Some(b'b') => out.push('\u{8}'),
                        Some(b'f') => out.push('\u{c}'),
                        Some(b'n') => out.push('\n'),
                        Some(b'r') => out.push('\r'),
                        Some(b't') => out.push('\t'),
                        Some(b'u') => {
                            self.i += 1;
                            let cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) {
                                // high surrogate: pair with a following low surrogate
                                if self.b[self.i..].starts_with(b"\\u") {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        let c =
                                            0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(c).unwrap_or('\u{FFFD}'));
                                    } else {
                                        out.push('\u{FFFD}');
                                        out.push(char::from_u32(lo).unwrap_or('\u{FFFD}'));
                                    }
                                } else {
                                    out.push('\u{FFFD}');
                                }
                            } else if (0xDC00..0xE000).contains(&cp) {
                                out.push('\u{FFFD}');
                            } else {
                                out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                            }
                        }
                        _ => return Err(format!("bad escape at byte {}", self.i)),
                    }
                    self.i += 1;
                }
                Some(_) => {
                    // copy one UTF-8 scalar as-is
                    let rest = match std::str::from_utf8(&self.b[self.i..]) {
                        Ok(s) => s,
                        Err(e) if e.valid_up_to() > 0 => std::str::from_utf8(
                            &self.b[self.i..self.i + e.valid_up_to()],
                        )
                        .map_err(|_| "invalid utf-8".to_string())?,
                        Err(_) => return Err("invalid utf-8".into()),
                    };
                    let c = rest.chars().next().ok_or("invalid utf-8")?;
                    out.push(c);
                    self.i += c.len_utf8();
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.i + 4 > self.b.len() {
            return Err("truncated \\u escape".into());
        }
        let s = std::str::from_utf8(&self.b[self.i..self.i + 4])
            .map_err(|_| "bad \\u escape".to_string())?;
        let v = u32::from_str_radix(s, 16).map_err(|_| "bad \\u escape".to_string())?;
        self.i += 4;
        Ok(v)
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.i += 1;
        }
        let mut float = false;
        if self.peek() == Some(b'.') {
            float = true;
            self.i += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            float = true;
            self.i += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        let s = std::str::from_utf8(&self.b[start..self.i])
            .map_err(|_| "bad number".to_string())?;
        if s.is_empty() || s == "-" {
            return Err(format!("bad number at byte {start}"));
        }
        if !float {
            if let Ok(v) = s.parse::<i64>() {
                return Ok(Json::Int(v));
            }
        }
        s.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("bad number '{s}'"))
    }
}

fn write_json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl std::fmt::Display for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::new();
        self.write(&mut s);
        f.write_str(&s)
    }
}

impl Json {
    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(i) => out.push_str(&i.to_string()),
            Json::Num(n) => {
                if n.is_finite() {
                    out.push_str(&n.to_string());
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => write_json_str(s, out),
            Json::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            Json::Obj(v) => {
                out.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_str(k, out);
                    out.push(':');
                    x.write(out);
                }
                out.push('}');
            }
        }
    }
}

pub fn jstr(s: impl Into<String>) -> Json {
    Json::Str(s.into())
}

pub fn jint(i: i64) -> Json {
    Json::Int(i)
}

pub fn jobj(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub fn jarr(v: Vec<Json>) -> Json {
    Json::Arr(v)
}

// ---------------------------------------------------------------------------
// line index: Wlel spans (1-based line, 1-based BYTE column) <-> LSP positions
// (0-based line, 0-based UTF-16 code unit offset)
// ---------------------------------------------------------------------------

pub struct LineIndex {
    text: String,
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex { text: text.to_string(), line_starts }
    }

    fn line_range(&self, line0: usize) -> (usize, usize) {
        let n = self.line_starts.len();
        let line0 = line0.min(n - 1);
        let start = self.line_starts[line0];
        let end = if line0 + 1 < n { self.line_starts[line0 + 1] } else { self.text.len() };
        (start, end)
    }

    /// byte offset within the line, floored to a char boundary
    fn clamp_col(&self, line0: usize, col1: usize) -> (usize, usize) {
        let (ls, le) = self.line_range(line0);
        let mut off = col1.saturating_sub(1).min(le - ls);
        while off > 0 && !self.text.is_char_boundary(ls + off) {
            off -= 1;
        }
        (ls, off)
    }

    /// Wlel `Pos` (1-based line/col) -> LSP (0-based line, UTF-16 character)
    pub fn to_lsp(&self, p: Pos) -> (u32, u32) {
        let line0 = p.line.saturating_sub(1);
        let (ls, off) = self.clamp_col(line0, p.col);
        let units: usize =
            self.text[ls..ls + off].chars().map(|c| c.len_utf16()).sum();
        (line0 as u32, units as u32)
    }

    /// LSP position -> Wlel `Pos` (clamped into the document)
    pub fn to_wlel(&self, line: u32, character: u32) -> Pos {
        let line0 = (line as usize).min(self.line_starts.len() - 1);
        let (ls, le) = self.line_range(line0);
        let mut units = 0usize;
        let mut byte_off = 0usize;
        for c in self.text[ls..le].chars() {
            if units >= character as usize {
                break;
            }
            units += c.len_utf16();
            byte_off += c.len_utf8();
        }
        Pos { line: line0 + 1, col: byte_off + 1 }
    }

    /// identifier under the cursor (expands across `[A-Za-z0-9_]`)
    pub fn word_at(&self, p: Pos) -> Option<(String, Span)> {
        let line0 = p.line.saturating_sub(1);
        let (ls, le) = self.line_range(line0);
        let bytes = self.text.as_bytes();
        let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut off = p.col.saturating_sub(1).min(le - ls);
        if off >= le - ls || !is_word(bytes[ls + off]) {
            if off == 0 || !is_word(bytes[ls + off - 1]) {
                return None;
            }
            off -= 1;
        }
        let mut a = off;
        while a > 0 && is_word(bytes[ls + a - 1]) {
            a -= 1;
        }
        let mut b = off;
        while b < le - ls && is_word(bytes[ls + b]) {
            b += 1;
        }
        let word = self.text[ls + a..ls + b].to_string();
        Some((word, Span::new(line0 + 1, a + 1, line0 + 1, b + 1)))
    }

    /// first whole-word occurrence of `word` inside `span` (used to narrow a
    /// declaration span down to the declared name)
    pub fn find_word(&self, span: Span, word: &str) -> Option<Span> {
        let first = span.start.line.saturating_sub(1);
        let last = span.end.line.saturating_sub(1);
        for line0 in first..=last.min(self.line_starts.len() - 1) {
            let (ls, le) = self.line_range(line0);
            let line = &self.text[ls..le];
            let bytes = line.as_bytes();
            let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
            let mut from = 0usize;
            while let Some(rel) = line[from..].find(word) {
                let a = from + rel;
                let b = a + word.len();
                let ok_before = a == 0 || !is_word(bytes[a - 1]);
                let ok_after = b >= bytes.len() || !is_word(bytes[b]);
                let in_span_col = if line0 + 1 == span.start.line {
                    a + 1 >= span.start.col
                } else {
                    true
                };
                if ok_before && ok_after && in_span_col {
                    return Some(Span::new(line0 + 1, a + 1, line0 + 1, b + 1));
                }
                from = a + 1;
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// analysis: parse + import merge + std splice + check
// ---------------------------------------------------------------------------

/// one diagnostic: severity 1 = error, 2 = warning (LSP DiagnosticSeverity)
#[derive(Debug, Clone)]
pub struct Diag {
    pub span: Span,
    pub msg: String,
    pub severity: u8,
}

/// a fully analyzed document: `program` is the post-check (typed) AST,
/// `index` is the pre-check clone that still has generic templates and the
/// raw `impl` blocks (method lookup uses it)
pub struct Analysis {
    pub program: Program,
    pub index: Program,
    pub diagnostics: Vec<Diag>,
    /// canonical file label every buffer decl was tagged with
    pub file: String,
}

fn empty_program() -> Program {
    Program {
        uses: Vec::new(),
        structs: Vec::new(),
        enums: Vec::new(),
        impls: Vec::new(),
        funcs: Vec::new(),
        tests: Vec::new(),
    }
}

/// analyze a document. `doc_path` enables file-import resolution and is used
/// to attribute the buffer's own declarations (None for unsaved buffers).
pub fn analyze(text: &str, doc_path: Option<&Path>) -> Analysis {
    let mut diags: Vec<Diag> = Vec::new();
    let toks = match Lexer::new(text).tokenize() {
        Ok(t) => t,
        Err(e) => {
            diags.push(Diag { span: Span::point(e.line, e.col), msg: e.msg, severity: 1 });
            return Analysis {
                program: empty_program(),
                index: empty_program(),
                diagnostics: diags,
                file: "<buffer>".into(),
            };
        }
    };
    let (mut program, parse_errors) = Parser::new(&toks).program();
    for e in &parse_errors {
        diags.push(Diag {
            span: e.span,
            msg: format!("syntax error: {}", e.msg),
            severity: 1,
        });
    }

    let file_str = doc_path
        .and_then(|p| p.canonicalize().ok())
        .or_else(|| doc_path.map(|p| p.to_path_buf()))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<buffer>".to_string());
    for f in program.funcs.iter_mut() {
        f.file = file_str.clone();
    }
    for s in program.structs.iter_mut() {
        s.file = file_str.clone();
    }
    for e in program.enums.iter_mut() {
        e.file = file_str.clone();
    }
    for i in program.impls.iter_mut() {
        i.file = file_str.clone();
    }
    for t in program.tests.iter_mut() {
        t.file = file_str.clone();
    }

    // merge `use "path.wl"` imports from disk — same semantics as
    // project::load_program (recursive, diamond-dedup, imports first)
    let mut sub_structs: Vec<StructDef> = Vec::new();
    let mut sub_enums: Vec<EnumDef> = Vec::new();
    let mut sub_impls: Vec<ImplDef> = Vec::new();
    let mut sub_funcs: Vec<FuncDef> = Vec::new();
    let mut sub_tests: Vec<TestDef> = Vec::new();
    match doc_path {
        Some(dp) => {
            let dir =
                dp.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
            let mut visited: HashSet<PathBuf> = HashSet::new();
            if let Ok(c) = dp.canonicalize() {
                visited.insert(c);
            }
            let use_count = program.uses.len();
            let mut propagated: Vec<UseDecl> = Vec::new();
            for ui in 0..use_count {
                let Some(rel) = program.uses[ui].path.clone() else { continue };
                let use_span = program.uses[ui].span;
                let ip = dir.join(&rel);
                match load_program(&ip, &mut visited) {
                    Ok(sub) => {
                        program.uses[ui].resolved =
                            ip.canonicalize().ok().map(|p| p.display().to_string());
                        // `use std` anywhere in the import tree pulls in the
                        // embedded library (loader parity)
                        if sub.uses.iter().any(|su| su.path.is_none()) {
                            propagated.push(UseDecl {
                                path: None,
                                resolved: None,
                                span: use_span,
                            });
                        }
                        sub_structs.extend(sub.structs);
                        sub_enums.extend(sub.enums);
                        sub_impls.extend(sub.impls);
                        sub_funcs.extend(sub.funcs);
                        sub_tests.extend(sub.tests);
                    }
                    Err(e) => diags.push(Diag {
                        span: use_span,
                        msg: format!("import: {e}"),
                        severity: 1,
                    }),
                }
            }
            program.uses.extend(propagated);
        }
        None => {
            for u in &program.uses {
                if u.path.is_some() {
                    diags.push(Diag {
                        span: u.span,
                        msg: "import: document is not saved to disk".into(),
                        severity: 1,
                    });
                }
            }
        }
    }
    let mut structs = sub_structs;
    structs.append(&mut program.structs);
    program.structs = structs;
    let mut enums = sub_enums;
    enums.append(&mut program.enums);
    program.enums = enums;
    let mut impls = sub_impls;
    impls.append(&mut program.impls);
    program.impls = impls;
    let mut funcs = sub_funcs;
    funcs.append(&mut program.funcs);
    program.funcs = funcs;
    let mut tests = sub_tests;
    tests.append(&mut program.tests);
    program.tests = tests;

    let has_std = program.uses.iter().any(|u| u.path.is_none());
    if has_std {
        splice_std(&mut program);
    }
    let index = program.clone();
    match Checker::check(&mut program) {
        Ok(warnings) => {
            for w in warnings {
                diags.push(Diag { span: w.span, msg: w.msg, severity: 2 });
            }
        }
        Err(e) => diags.push(Diag { span: e.span, msg: e.msg, severity: 1 }),
    }
    Analysis { program, index, diagnostics: diags, file: file_str }
}

// ---------------------------------------------------------------------------
// positions, name lookup helpers
// ---------------------------------------------------------------------------

fn pos_le(a: Pos, b: Pos) -> bool {
    a.line < b.line || (a.line == b.line && a.col <= b.col)
}

fn pos_in(s: Span, p: Pos) -> bool {
    pos_le(s.start, p) && pos_le(p, s.end)
}

fn span_size(s: Span) -> usize {
    (s.end.line.saturating_sub(s.start.line)) * 1_000_000
        + s.end.col.saturating_sub(s.start.col)
}

fn strip_ptr(n: &str) -> &str {
    n.strip_prefix('*').unwrap_or(n)
}

/// type annotation as written: drop generic args ("Box[int]" -> "Box")
fn strip_brackets(n: &str) -> &str {
    match n.find('[') {
        Some(i) => &n[..i],
        None => n,
    }
}

/// mangled monomorphized name back to its template ("Vec__int" -> "Vec");
/// exact names always win over this heuristic
fn origin_type_name(n: &str) -> &str {
    n.split("__").next().unwrap_or(n)
}

#[derive(Debug, Clone)]
struct LocalDef {
    name: String,
    ty: Option<String>,
    kind: &'static str,
    span: Span,
}

/// every binding visible at `pos` inside the containing function/test
/// (outer scopes first, shadowing bindings later — `.rev().find` picks the
/// visible one)
fn locals_at(a: &Analysis, pos: Pos) -> Vec<LocalDef> {
    let mut out: Vec<LocalDef> = Vec::new();
    let mut visited = false;
    for f in a.program.funcs.iter().filter(|f| f.file == a.file) {
        if pos_in(f.span, pos) {
            for p in &f.params {
                if pos_le(p.span.start, pos) {
                    out.push(LocalDef {
                        name: p.name.clone(),
                        ty: p.ty.clone(),
                        kind: "parameter",
                        span: p.span,
                    });
                }
            }
            collect_body(&f.body.0, pos, &mut out);
            visited = true;
            break;
        }
    }
    if !visited {
        for t in a.program.tests.iter().filter(|t| t.file == a.file) {
            if pos_in(t.span, pos) {
                collect_body(&t.body.0, pos, &mut out);
                break;
            }
        }
    }
    out
}

fn record_stmt_binding(st: &Stmt, out: &mut Vec<LocalDef>) {
    match &st.node {
        StmtKind::Let(name, ty_ann, _) => out.push(LocalDef {
            name: name.clone(),
            ty: ty_ann.clone(),
            kind: "local",
            span: st.span,
        }),
        StmtKind::For(var, _, _, _) => out.push(LocalDef {
            name: var.clone(),
            ty: Some("int".into()),
            kind: "loop variable",
            span: st.span,
        }),
        StmtKind::ForIn(fi) => out.push(LocalDef {
            name: fi.var.clone(),
            ty: fi.elem.clone(),
            kind: "loop variable",
            span: st.span,
        }),
        _ => {}
    }
}

fn collect_body(stmts: &[Stmt], pos: Pos, out: &mut Vec<LocalDef>) {
    for st in stmts {
        if pos_in(st.span, pos) {
            // the statement under the cursor: its binding is already usable
            // (e.g. cursor right after `x := `), then descend into it
            record_stmt_binding(st, out);
            collect_in_stmt(st, pos, out);
        } else if pos_le(st.span.end, pos) {
            // entirely before the cursor: binding visible, block-scoped
            // children of a finished sibling are NOT
            record_stmt_binding(st, out);
        }
    }
}

fn collect_in_stmt(st: &Stmt, pos: Pos, out: &mut Vec<LocalDef>) {
    match &st.node {
        StmtKind::If(i) => collect_if(i, pos, out),
        StmtKind::While(_, b) | StmtKind::Arena(_, b) => collect_body(&b.0, pos, out),
        StmtKind::For(_, _, _, b) => collect_body(&b.0, pos, out),
        StmtKind::ForIn(fi) => collect_body(&fi.body.0, pos, out),
        StmtKind::Defer(DeferBody::Block(b)) => collect_body(&b.0, pos, out),
        StmtKind::Block(b) => collect_body(&b.0, pos, out),
        StmtKind::Match(m) => collect_match_arms(m, pos, out),
        // value-form match hides inside these statements' expressions
        StmtKind::Return(Some(e)) | StmtKind::Let(_, _, e) | StmtKind::ExprStmt(e) => {
            collect_match_in_expr(e, pos, out)
        }
        _ => {}
    }
}

fn collect_match_arms(m: &MatchStmt, pos: Pos, out: &mut Vec<LocalDef>) {
    for arm in &m.arms {
        if pos_in(arm.span, pos) {
            if let Pattern::Variant { binds, .. } = &arm.pattern {
                for b in binds {
                    out.push(LocalDef {
                        name: b.name.clone(),
                        ty: b.ty.clone(),
                        kind: "pattern binding",
                        span: arm.span,
                    });
                }
            }
            if let MatchBody::Block(b) = &arm.body {
                collect_body(&b.0, pos, out);
            }
        }
    }
}

fn collect_match_in_expr(e: &Expr, pos: Pos, out: &mut Vec<LocalDef>) {
    match &e.node {
        ExprKind::Match(m) => collect_match_arms(m, pos, out),
        ExprKind::Try(t) => collect_match_in_expr(&t.inner, pos, out),
        _ => {}
    }
}

fn collect_if(i: &IfStmt, pos: Pos, out: &mut Vec<LocalDef>) {
    collect_body(&i.then_body.0, pos, out);
    match &i.else_branch {
        Some(ElseBranch::If(e)) => collect_if(e, pos, out),
        Some(ElseBranch::Block(b)) => collect_body(&b.0, pos, out),
        None => {}
    }
}

// ---------------------------------------------------------------------------
// symbol index helpers (lookups over the pre-check "index" program)
// ---------------------------------------------------------------------------

fn find_fn<'a>(a: &'a Analysis, name: &str) -> Option<&'a FuncDef> {
    a.index.funcs.iter().find(|f| f.name == name)
}

fn find_struct<'a>(a: &'a Analysis, name: &str) -> Option<&'a StructDef> {
    a.index.structs.iter().find(|s| s.name == name)
}

fn find_enum<'a>(a: &'a Analysis, name: &str) -> Option<&'a EnumDef> {
    a.index.enums.iter().find(|e| e.name == name)
}

fn find_variant<'a>(a: &'a Analysis, name: &str) -> Option<(&'a EnumDef, &'a VariantDef)> {
    for e in &a.index.enums {
        if let Some(v) = e.variants.iter().find(|v| v.name == name) {
            return Some((e, v));
        }
    }
    None
}

/// method by (owner type, name); returns the impl for its `file`
fn find_method<'a>(
    a: &'a Analysis,
    type_name: &str,
    method: &str,
) -> Option<(&'a ImplDef, &'a FuncDef)> {
    let imp = a.index.impls.iter().find(|i| i.type_name == type_name)?;
    let m = imp.methods.iter().find(|m| m.name == method)?;
    Some((imp, m))
}

/// the checker rewrites `recv.method(args)` into a call to the desugared
/// function (`Pt__len(recv, ...)`, monomorphized: `Box__get__int(...)`) —
/// recover the (impl, method) pair from such a call name
fn method_of_call<'a>(a: &'a Analysis, call_name: &str) -> Option<(&'a ImplDef, &'a FuncDef)> {
    for imp in &a.index.impls {
        for m in &imp.methods {
            let full = format!("{}__{}", imp.type_name, m.name);
            if call_name == full || call_name.starts_with(&format!("{full}__")) {
                return Some((imp, m));
            }
        }
    }
    None
}

/// struct lookup that understands concrete/monomorphized names
/// ("Vec__int"): exact match first, then the template origin
fn struct_by_concrete<'a>(a: &'a Analysis, name: &str) -> Option<&'a StructDef> {
    a.program
        .structs
        .iter()
        .find(|s| s.name == name)
        .or_else(|| {
            let origin = origin_type_name(name);
            a.program.structs.iter().find(|s| s.name == origin)
        })
        .or_else(|| find_struct(a, strip_brackets(name)))
}

// ---------------------------------------------------------------------------
// signature rendering
// ---------------------------------------------------------------------------

fn fn_sig(f: &FuncDef) -> String {
    let mut s = String::new();
    if f.is_extern {
        s.push_str("extern ");
    }
    s.push_str("fn ");
    s.push_str(&f.name);
    if !f.type_params.is_empty() {
        s.push_str(&format!("[{}]", f.type_params.join(", ")));
    }
    s.push('(');
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&p.name);
        if let Some(t) = &p.ty {
            s.push_str(&format!(": {t}"));
        }
    }
    s.push(')');
    if let Some(r) = &f.ret_type {
        s.push_str(&format!(" -> {r}"));
    }
    s
}

fn method_sig(m: &FuncDef) -> String {
    let mut s = String::from("fn ");
    s.push_str(&m.name);
    s.push('(');
    for (i, p) in m.params.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&p.name);
        if let Some(t) = &p.ty {
            s.push_str(&format!(": {t}"));
        }
    }
    s.push(')');
    if let Some(r) = &m.ret_type {
        s.push_str(&format!(" -> {r}"));
    }
    s
}

fn struct_sig(s: &StructDef) -> String {
    let mut head = format!("struct {}", s.name);
    if !s.type_params.is_empty() {
        head.push_str(&format!("[{}]", s.type_params.join(", ")));
    }
    let fields = s
        .fields
        .iter()
        .map(|(n, t)| format!("{n}: {t}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{head} {{ {fields} }}")
}

fn enum_sig(e: &EnumDef) -> String {
    let mut head = format!("enum {}", e.name);
    if !e.type_params.is_empty() {
        head.push_str(&format!("[{}]", e.type_params.join(", ")));
    }
    let variants = e
        .variants
        .iter()
        .map(|v| {
            if v.payloads.is_empty() {
                v.name.clone()
            } else {
                format!("{}({})", v.name, v.payloads.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{head} {{ {variants} }}")
}

fn variant_sig(v: &VariantDef) -> String {
    if v.payloads.is_empty() {
        v.name.clone()
    } else {
        format!("{}({})", v.name, v.payloads.join(", "))
    }
}

fn hover_md(code: &str, note: &str) -> String {
    if note.is_empty() {
        format!("```wl\n{code}\n```")
    } else {
        format!("```wl\n{code}\n```\n{note}")
    }
}

fn primitive_type(word: &str) -> bool {
    matches!(
        word,
        "int" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize"
            | "float" | "f32" | "f64" | "bool" | "string" | "byte" | "char" | "void"
    )
}

// ---------------------------------------------------------------------------
// innermost expression at a position
// ---------------------------------------------------------------------------

fn best_expr_at<'a>(a: &'a Analysis, pos: Pos) -> Option<&'a Expr> {
    let mut best: Option<&'a Expr> = None;
    for f in a.program.funcs.iter().filter(|f| f.file == a.file) {
        if pos_in(f.span, pos) {
            walk_stmts(&f.body.0, pos, &mut best);
            return best;
        }
    }
    for t in a.program.tests.iter().filter(|t| t.file == a.file) {
        if pos_in(t.span, pos) {
            walk_stmts(&t.body.0, pos, &mut best);
            return best;
        }
    }
    best
}

fn consider<'a>(e: &'a Expr, pos: Pos, best: &mut Option<&'a Expr>) {
    if pos_in(e.span, pos) && best.is_none_or(|b| span_size(e.span) < span_size(b.span)) {
        *best = Some(e);
    }
}

fn walk_expr_rec<'a>(e: &'a Expr, pos: Pos, best: &mut Option<&'a Expr>) {
    if !pos_in(e.span, pos) {
        return;
    }
    consider(e, pos, best);
    match &e.node {
        ExprKind::Unary(_, x) | ExprKind::AddrOf(x) | ExprKind::Deref(x) => {
            walk_expr_rec(x, pos, best)
        }
        ExprKind::Binary(_, l, r) => {
            walk_expr_rec(l, pos, best);
            walk_expr_rec(r, pos, best);
        }
        ExprKind::Call(_, _, args) => {
            for a in args {
                walk_expr_rec(a, pos, best);
            }
        }
        ExprKind::MethodCall(recv, _, args) => {
            walk_expr_rec(recv, pos, best);
            for a in args {
                walk_expr_rec(a, pos, best);
            }
        }
        ExprKind::Field(obj, _) => walk_expr_rec(obj, pos, best),
        ExprKind::StructLit(_, fields) => {
            for (_, v) in fields {
                walk_expr_rec(v, pos, best);
            }
        }
        ExprKind::Cast(_, x) => walk_expr_rec(x, pos, best),
        ExprKind::Index(a, b) => {
            walk_expr_rec(a, pos, best);
            walk_expr_rec(b, pos, best);
        }
        ExprKind::ArrayLit(v) => {
            for x in v {
                walk_expr_rec(x, pos, best);
            }
        }
        ExprKind::New(_, Some(n)) => walk_expr_rec(n, pos, best),
        ExprKind::Match(m) => walk_match(m, pos, best),
        ExprKind::Try(t) => walk_expr_rec(&t.inner, pos, best),
        _ => {}
    }
}

fn walk_match<'a>(m: &'a MatchStmt, pos: Pos, best: &mut Option<&'a Expr>) {
    walk_expr_rec(&m.scrutinee, pos, best);
    for arm in &m.arms {
        if pos_in(arm.span, pos) {
            match &arm.body {
                MatchBody::Expr(e) => walk_expr_rec(e, pos, best),
                MatchBody::Block(b) => walk_stmts(&b.0, pos, best),
            }
        }
    }
}

/// is `pos` on a match arm's pattern (before the `=>` body)?
fn match_pattern_region(m: &MatchStmt, pos: Pos) -> bool {
    for arm in &m.arms {
        let body_start = match &arm.body {
            MatchBody::Expr(e) => e.span.start,
            // Block carries no span: its first statement marks the body start
            MatchBody::Block(b) => b
                .0
                .first()
                .map(|st| st.span.start)
                .unwrap_or(arm.span.end),
        };
        if pos_le(arm.span.start, pos) && pos_le(pos, body_start) {
            return true;
        }
    }
    false
}

fn walk_stmts<'a>(stmts: &'a [Stmt], pos: Pos, best: &mut Option<&'a Expr>) {
    for st in stmts {
        if !pos_in(st.span, pos) {
            continue;
        }
        match &st.node {
            StmtKind::Let(_, _, e) => walk_expr_rec(e, pos, best),
            StmtKind::Assign(t) => {
                walk_expr_rec(&t.target, pos, best);
                walk_expr_rec(&t.value, pos, best);
            }
            StmtKind::If(i) => walk_if(i, pos, best),
            StmtKind::While(c, b) => {
                walk_expr_rec(c, pos, best);
                walk_stmts(&b.0, pos, best);
            }
            StmtKind::For(_, s, e, b) => {
                walk_expr_rec(s, pos, best);
                walk_expr_rec(e, pos, best);
                walk_stmts(&b.0, pos, best);
            }
            StmtKind::ForIn(fi) => {
                walk_expr_rec(&fi.iter, pos, best);
                walk_stmts(&fi.body.0, pos, best);
            }
            StmtKind::Return(Some(e)) => walk_expr_rec(e, pos, best),
            StmtKind::ExprStmt(e) => walk_expr_rec(e, pos, best),
            StmtKind::Defer(DeferBody::Expr(e)) => walk_expr_rec(e, pos, best),
            StmtKind::Defer(DeferBody::Block(b)) => walk_stmts(&b.0, pos, best),
            StmtKind::Arena(size, b) => {
                if let Some(s) = size {
                    walk_expr_rec(s, pos, best);
                }
                walk_stmts(&b.0, pos, best);
            }
            StmtKind::Match(m) => walk_match(m, pos, best),
            StmtKind::Block(b) => walk_stmts(&b.0, pos, best),
            _ => {}
        }
    }
}

fn walk_if<'a>(i: &'a IfStmt, pos: Pos, best: &mut Option<&'a Expr>) {
    walk_expr_rec(&i.cond, pos, best);
    walk_stmts(&i.then_body.0, pos, best);
    match &i.else_branch {
        Some(ElseBranch::If(e)) => walk_if(e, pos, best),
        Some(ElseBranch::Block(b)) => walk_stmts(&b.0, pos, best),
        None => {}
    }
}

// ---------------------------------------------------------------------------
// hover
// ---------------------------------------------------------------------------

/// hover info at a position: (markdown contents, span to highlight)
pub fn hover(a: &Analysis, idx: &LineIndex, pos: Pos) -> Option<(String, Span)> {
    if let Some(e) = best_expr_at(a, pos) {
        // hovering a match arm's PATTERN region: no expression covers it —
        // fall through to the word fallback, which resolves variant names
        // and pattern bindings through the locals collector
        let on_pattern = match &e.node {
            ExprKind::Match(m) => match_pattern_region(m, pos),
            ExprKind::Try(t) => {
                matches!(&t.inner.node, ExprKind::Match(m) if match_pattern_region(m, pos))
            }
            _ => false,
        };
        if !on_pattern {
            if let Some((code, note)) = expr_hover(a, e) {
                return Some((hover_md(&code, &note), e.span));
            }
        }
    }
    // fallback: the word under the cursor (annotations, params, pattern
    // binds, type names in casts/struct literals)
    let (word, wsp) = idx.word_at(pos)?;
    if let Some(l) = locals_at(a, pos).into_iter().rev().find(|l| l.name == word) {
        let ty = l.ty.clone().unwrap_or_else(|| "?".into());
        return Some((hover_md(&format!("{word}: {ty}"), l.kind), wsp));
    }
    if let Some(f) = find_fn(a, &word) {
        return Some((hover_md(&fn_sig(f), "function"), f.span));
    }
    if let Some(s) = find_struct(a, &word) {
        return Some((hover_md(&struct_sig(s), "struct"), s.span));
    }
    if let Some(e) = find_enum(a, &word) {
        return Some((hover_md(&enum_sig(e), "enum"), e.span));
    }
    if let Some((e, v)) = find_variant(a, &word) {
        return Some((
            hover_md(&variant_sig(v), &format!("variant of {}", e.name)),
            e.span,
        ));
    }
    if primitive_type(&word) {
        return Some((hover_md(&word, "primitive type"), wsp));
    }
    None
}

fn expr_hover(a: &Analysis, e: &Expr) -> Option<(String, String)> {
    match &e.node {
        ExprKind::Ident(n) => {
            let ty = e.ty.as_ref()?;
            Some((format!("{n}: {ty}"), String::new()))
        }
        ExprKind::Call(name, _, _) => {
            // desugared method call first (`Pt__len`, `Box__get__int`)
            if let Some((imp, m)) = method_of_call(a, name) {
                return Some((method_sig(m), format!("method of {}", imp.type_name)));
            }
            // generic calls are rewritten to monomorphized names
            // (`vec_new[int]()` -> `vec_new__int`): fall back to the template
            if let Some(f) = find_fn(a, name).or_else(|| find_fn(a, origin_type_name(name))) {
                return Some((fn_sig(f), "function".into()));
            }
            if let Some((en, v)) = find_variant(a, name) {
                return Some((variant_sig(v), format!("variant of {}", en.name)));
            }
            let ty = e.ty.as_ref()?;
            let note = if name.contains("::") { "builtin" } else { "call" };
            Some((format!("{name}(...) -> {ty}"), note.into()))
        }
        ExprKind::MethodCall(recv, name, _) => {
            let rt = recv.ty.as_deref()?;
            let owner = origin_type_name(strip_ptr(strip_brackets(rt)));
            let (_imp, m) = find_method(a, owner, name)?;
            Some((method_sig(m), format!("method of {owner}")))
        }
        // variant constructor: the checker rewrote the call into an EnumLit
        ExprKind::EnumLit(_, _, variant, _) => {
            if let Some((en, v)) = find_variant(a, variant) {
                return Some((variant_sig(v), format!("variant of {}", en.name)));
            }
            e.ty.as_ref().map(|t| (t.clone(), String::new()))
        }
        ExprKind::Field(obj, name) => {
            let ot = obj.ty.as_deref()?;
            let s = struct_by_concrete(a, strip_ptr(ot))?;
            let (_, fty) = s.fields.iter().find(|(n, _)| n == name)?;
            Some((
                format!("{name}: {fty}"),
                format!("field of {}", s.name),
            ))
        }
        ExprKind::StructLit(name, _) => {
            let s = struct_by_concrete(a, name)?;
            Some((struct_sig(s), "struct".into()))
        }
        ExprKind::Cast(t, _) | ExprKind::New(t, _) | ExprKind::Sizeof(t) => {
            Some((t.clone(), "type".into()))
        }
        _ => e.ty.as_ref().map(|t| (t.clone(), String::new())),
    }
}

// ---------------------------------------------------------------------------
// go-to-definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct DefSite {
    pub file: String,
    pub span: Span,
}

/// narrow a declaration span to the declared name when the declaration lives
/// in the current document (text scan); other files keep their start point
fn site_of(a: &Analysis, idx: &LineIndex, file: &str, span: Span, word: &str) -> DefSite {
    let span = if file == a.file {
        idx.find_word(span, word)
            .unwrap_or_else(|| Span::point(span.start.line, span.start.col))
    } else {
        Span::point(span.start.line, span.start.col)
    };
    DefSite { file: file.to_string(), span }
}

fn type_def(a: &Analysis, idx: &LineIndex, tname: &str) -> Option<DefSite> {
    let short = strip_brackets(strip_ptr(tname));
    if let Some(s) = find_struct(a, short) {
        return Some(site_of(a, idx, &s.file, s.span, &s.name));
    }
    if let Some(e) = find_enum(a, short) {
        return Some(site_of(a, idx, &e.file, e.span, &e.name));
    }
    None
}

fn top_def(a: &Analysis, idx: &LineIndex, name: &str) -> Option<DefSite> {
    if let Some(f) = find_fn(a, name) {
        return Some(site_of(a, idx, &f.file, f.span, &f.name));
    }
    if let Some(s) = find_struct(a, name) {
        return Some(site_of(a, idx, &s.file, s.span, &s.name));
    }
    if let Some(e) = find_enum(a, name) {
        return Some(site_of(a, idx, &e.file, e.span, &e.name));
    }
    if let Some((e, _)) = find_variant(a, name) {
        return Some(site_of(a, idx, &e.file, e.span, name));
    }
    None
}

/// definition site at a position, if resolvable
pub fn definition(a: &Analysis, idx: &LineIndex, pos: Pos) -> Option<DefSite> {
    if let Some(e) = best_expr_at(a, pos) {
        match &e.node {
            ExprKind::Ident(n) => {
                if let Some(l) = locals_at(a, pos).into_iter().rev().find(|l| &l.name == n) {
                    let sp = idx
                        .find_word(l.span, &l.name)
                        .unwrap_or_else(|| Span::point(l.span.start.line, l.span.start.col));
                    return Some(DefSite { file: a.file.clone(), span: sp });
                }
                return top_def(a, idx, n);
            }
            ExprKind::Call(name, _, _) => {
                if let Some((imp, m)) = method_of_call(a, name) {
                    return Some(site_of(a, idx, &imp.file, m.span, &m.name));
                }
                if let Some(f) = find_fn(a, name).or_else(|| find_fn(a, origin_type_name(name))) {
                    return Some(site_of(a, idx, &f.file, f.span, &f.name));
                }
                return top_def(a, idx, name);
            }
            ExprKind::MethodCall(recv, name, _) => {
                let rt = recv.ty.as_deref()?;
                let owner = origin_type_name(strip_ptr(strip_brackets(rt)));
                let (imp, m) = find_method(a, owner, name)?;
                return Some(site_of(a, idx, &imp.file, m.span, &m.name));
            }
            ExprKind::Field(obj, name) => {
                let ot = obj.ty.as_deref()?;
                let s = struct_by_concrete(a, strip_ptr(ot))?;
                // field declarations have no span of their own: locate the
                // field name inside the struct body by text scan; fall back
                // to the struct definition (also for cross-file structs)
                if s.file == a.file {
                    if let Some(sp) = idx.find_word(s.span, name) {
                        return Some(DefSite { file: a.file.clone(), span: sp });
                    }
                }
                return Some(site_of(a, idx, &s.file, s.span, &s.name));
            }
            ExprKind::StructLit(name, _) => return type_def(a, idx, name),
            // variant constructor (checker-rewritten call): jump to the
            // variant's line inside its enum declaration
            ExprKind::EnumLit(_, _, variant, _) => {
                if let Some((en, _)) = find_variant(a, variant) {
                    return Some(site_of(a, idx, &en.file, en.span, variant));
                }
            }
            ExprKind::Cast(t, _) | ExprKind::New(t, _) | ExprKind::Sizeof(t) => {
                return type_def(a, idx, t)
            }
            _ => {}
        }
    }
    // `use "path.wl"` under the cursor → open the imported file
    for u in &a.program.uses {
        if pos_in(u.span, pos) {
            if let Some(res) = &u.resolved {
                return Some(DefSite { file: res.clone(), span: Span::point(1, 1) });
            }
        }
    }
    let (word, _) = idx.word_at(pos)?;
    top_def(a, idx, &word)
}

// ---------------------------------------------------------------------------
// completion
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Compl {
    pub label: String,
    /// LSP CompletionItemKind (6 variable, 3 function, 22 struct, 23 enum,
    /// 20 enum member, 14 keyword)
    pub kind: i64,
    pub detail: String,
}

fn upsert(out: &mut Vec<Compl>, c: Compl) {
    match out.iter_mut().find(|e| e.label == c.label) {
        Some(e) => *e = c,
        None => out.push(c),
    }
}

/// completions at a position: locals first, then top-level symbols (std
/// included when spliced), then keywords and primitive types
pub fn completion(a: &Analysis, pos: Pos) -> Vec<Compl> {
    let mut out: Vec<Compl> = Vec::new();
    for l in locals_at(a, pos) {
        let detail = l.ty.unwrap_or_else(|| l.kind.to_string());
        upsert(&mut out, Compl { label: l.name, kind: 6, detail });
    }
    for f in &a.index.funcs {
        // monomorphized copies and desugared methods ("id__int", "Pt__len")
        // are internal spellings — the template is what the user writes
        if f.name.contains("__") {
            continue;
        }
        upsert(&mut out, Compl {
            label: f.name.clone(),
            kind: 3,
            detail: fn_sig(f),
        });
    }
    for s in &a.index.structs {
        upsert(&mut out, Compl {
            label: s.name.clone(),
            kind: 22,
            detail: struct_sig(s),
        });
    }
    for imp in &a.index.impls {
        for m in &imp.methods {
            upsert(&mut out, Compl {
                label: m.name.clone(),
                kind: 2,
                detail: format!("{}  (method of {})", method_sig(m), imp.type_name),
            });
        }
    }
    for e in &a.index.enums {
        upsert(&mut out, Compl { label: e.name.clone(), kind: 23, detail: enum_sig(e) });
        for v in &e.variants {
            upsert(&mut out, Compl {
                label: v.name.clone(),
                kind: 20,
                detail: format!("variant of {}", e.name),
            });
        }
    }
    for kw in [
        "fn", "extern", "struct", "enum", "impl", "match", "defer", "arena", "use",
        "test", "let", "return", "if", "else", "while", "for", "in", "break",
        "continue", "true", "false", "new", "as",
    ] {
        upsert(&mut out, Compl { label: kw.into(), kind: 14, detail: String::new() });
    }
    for ty in [
        "int", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "usize",
        "float", "f32", "f64", "bool", "string", "byte", "char", "void",
    ] {
        upsert(&mut out, Compl {
            label: ty.into(),
            kind: 14,
            detail: "primitive type".into(),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// LSP server (protocol logic is IO-free and unit-testable)
// ---------------------------------------------------------------------------

struct Doc {
    text: String,
    version: i64,
    path: Option<PathBuf>,
    analysis: Option<Analysis>,
}

pub struct Server {
    docs: HashMap<String, Doc>,
    /// set after the `exit` notification
    pub done: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

fn pos_json(l: u32, c: u32) -> Json {
    jobj(vec![("line", jint(l as i64)), ("character", jint(c as i64))])
}

fn range_json(sl: u32, sc: u32, el: u32, ec: u32) -> Json {
    jobj(vec![("start", pos_json(sl, sc)), ("end", pos_json(el, ec))])
}

fn diag_json(text: &str, dg: &Diag) -> Json {
    let idx = LineIndex::new(text);
    let (sl, sc) = idx.to_lsp(dg.span.start);
    let (el, mut ec) = idx.to_lsp(dg.span.end);
    if (el, ec) == (sl, sc) {
        // zero-width spans are invisible in editors: widen by one character
        ec += 1;
    }
    jobj(vec![
        ("range", range_json(sl, sc, el, ec)),
        ("severity", jint(dg.severity as i64)),
        ("source", jstr("wlel")),
        ("message", jstr(dg.msg.clone())),
    ])
}

/// percent-decode a URI path component
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Ok(v) = u8::from_str_radix(
                std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `file:///abs/path.wl` -> `/abs/path.wl` (percent-decoded)
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Windows drive form: file:///C:/x -> C:/x
    let rest = if rest.starts_with('/')
        && rest.len() >= 3
        && rest.as_bytes()[2] == b':'
    {
        &rest[1..]
    } else {
        rest
    };
    Some(PathBuf::from(percent_decode(rest)))
}

fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_'
            | b'~' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn file_to_uri(file: &str) -> String {
    if file == STD_FILE {
        // the embedded stdlib is not a file on disk; a private scheme keeps
        // the location honest (clients show it, cannot open it)
        return "wlel-std:stdlib.wl".into();
    }
    format!("file://{}", percent_encode_path(file))
}

fn initialize_result() -> Json {
    jobj(vec![
        (
            "capabilities",
            jobj(vec![
                (
                    "textDocumentSync",
                    jobj(vec![
                        ("openClose", Json::Bool(true)),
                        ("change", jint(1)), // full sync
                    ]),
                ),
                ("hoverProvider", Json::Bool(true)),
                ("definitionProvider", Json::Bool(true)),
                (
                    "completionProvider",
                    jobj(vec![(
                        "triggerCharacters",
                        jarr(vec![jstr(":"), jstr(".")]),
                    )]),
                ),
            ]),
        ),
        (
            "serverInfo",
            jobj(vec![
                ("name", jstr("wlel")),
                ("version", jstr(env!("CARGO_PKG_VERSION"))),
            ]),
        ),
    ])
}

impl Server {
    pub fn new() -> Self {
        Server { docs: HashMap::new(), done: false }
    }

    fn respond(&self, id: Option<Json>, result: Json) -> Json {
        jobj(vec![
            ("jsonrpc", jstr("2.0")),
            ("id", id.unwrap_or(Json::Null)),
            ("result", result),
        ])
    }

    fn respond_err(&self, id: Option<Json>, code: i64, message: &str) -> Json {
        jobj(vec![
            ("jsonrpc", jstr("2.0")),
            ("id", id.unwrap_or(Json::Null)),
            (
                "error",
                jobj(vec![("code", jint(code)), ("message", jstr(message))]),
            ),
        ])
    }

    fn notify(method: &str, params: Json) -> Json {
        jobj(vec![
            ("jsonrpc", jstr("2.0")),
            ("method", jstr(method)),
            ("params", params),
        ])
    }

    /// handle one incoming JSON-RPC message; returns every outgoing message
    /// (responses AND server notifications like publishDiagnostics, in order)
    pub fn handle(&mut self, msg: &Json) -> Vec<Json> {
        let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
            return Vec::new();
        };
        let id = msg.get("id").cloned();
        let params = msg.get("params");
        match method {
            "initialize" => vec![self.respond(id, initialize_result())],
            "initialized" | "textDocument/didSave" | "$/setTrace" | "$/logTrace" => {
                Vec::new()
            }
            "shutdown" => vec![self.respond(id, Json::Null)],
            "exit" => {
                self.done = true;
                Vec::new()
            }
            "textDocument/didOpen" => {
                let uri = self.open(params);
                uri.and_then(|u| self.publish(&u)).into_iter().collect()
            }
            "textDocument/didChange" => {
                let uri = self.change(params);
                uri.and_then(|u| self.publish(&u)).into_iter().collect()
            }
            "textDocument/didClose" => {
                let uri = params
                    .and_then(|p| p.ptr(&["textDocument", "uri"]))
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string());
                if let Some(u) = &uri {
                    self.docs.remove(u);
                    // editors expect a final empty publish after close
                    return vec![Self::notify(
                        "textDocument/publishDiagnostics",
                        jobj(vec![("uri", jstr(u.clone())), ("diagnostics", jarr(vec![]))]),
                    )];
                }
                Vec::new()
            }
            "textDocument/hover" => self.hover_req(params, id),
            "textDocument/definition" => self.definition_req(params, id),
            "textDocument/completion" => self.completion_req(params, id),
            other => {
                if other.starts_with("$/") {
                    Vec::new()
                } else if id.is_some() {
                    vec![self.respond_err(id, -32601, "Method not found")]
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn open(&mut self, params: Option<&Json>) -> Option<String> {
        let td = params.and_then(|p| p.get("textDocument"))?;
        let uri = td.get("uri").and_then(|u| u.as_str())?.to_string();
        let text = td.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let version = td.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
        let path = uri_to_path(&uri);
        let analysis = Some(analyze(&text, path.as_deref()));
        self.docs.insert(
            uri.clone(),
            Doc { text, version, path, analysis },
        );
        Some(uri)
    }

    fn change(&mut self, params: Option<&Json>) -> Option<String> {
        let td = params.and_then(|p| p.get("textDocument"))?;
        let uri = td.get("uri").and_then(|u| u.as_str())?.to_string();
        let version = td.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
        let changes = params.and_then(|p| p.get("contentChanges")).and_then(|c| c.as_arr())?;
        // full sync: the first range-less change carries the whole document
        let new_text = changes
            .iter()
            .find(|c| c.get("range").is_none())
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())?
            .to_string();
        let doc = self.docs.get_mut(&uri)?;
        doc.text = new_text;
        doc.version = version;
        doc.analysis = None;
        Some(uri)
    }

    fn publish(&self, uri: &str) -> Option<Json> {
        let d = self.docs.get(uri)?;
        let mut items: Vec<Json> = Vec::new();
        if let Some(a) = &d.analysis {
            for dg in &a.diagnostics {
                items.push(diag_json(&d.text, dg));
            }
        }
        Some(Self::notify(
            "textDocument/publishDiagnostics",
            jobj(vec![
                ("uri", jstr(uri)),
                ("version", jint(d.version)),
                ("diagnostics", jarr(items)),
            ]),
        ))
    }

    /// (uri, (line, character)) from request params
    fn doc_pos(params: Option<&Json>) -> Option<(String, (u32, u32))> {
        let uri = params
            .and_then(|p| p.ptr(&["textDocument", "uri"]))
            .and_then(|u| u.as_str())?
            .to_string();
        let p = params.and_then(|p| p.get("position"))?;
        let line = p.get("line").and_then(|v| v.as_i64())? as u32;
        let ch = p.get("character").and_then(|v| v.as_i64())? as u32;
        Some((uri, (line, ch)))
    }

    /// doc with its analysis (re-analyzed lazily after a change)
    fn doc_analysis(&mut self, uri: &str) -> Option<(&Doc, &Analysis)> {
        let d = self.docs.get_mut(uri)?;
        if d.analysis.is_none() {
            let text = d.text.clone();
            let path = d.path.clone();
            d.analysis = Some(analyze(&text, path.as_deref()));
        }
        let d = self.docs.get(uri)?;
        let a = d.analysis.as_ref()?;
        Some((d, a))
    }

    fn hover_req(&mut self, params: Option<&Json>, id: Option<Json>) -> Vec<Json> {
        let Some((uri, (line, ch))) = Self::doc_pos(params) else {
            return vec![self.respond_err(id, -32602, "invalid params")];
        };
        let Some((d, a)) = self.doc_analysis(&uri) else {
            return vec![self.respond(id, Json::Null)];
        };
        let idx = LineIndex::new(&d.text);
        let wl = idx.to_wlel(line, ch);
        match hover(a, &idx, wl) {
            Some((md, span)) => {
                let (sl, sc) = idx.to_lsp(span.start);
                let (el, ec) = idx.to_lsp(span.end);
                let result = jobj(vec![
                    (
                        "contents",
                        jobj(vec![("kind", jstr("markdown")), ("value", jstr(md))]),
                    ),
                    ("range", range_json(sl, sc, el, ec)),
                ]);
                vec![self.respond(id, result)]
            }
            None => vec![self.respond(id, Json::Null)],
        }
    }

    fn definition_req(&mut self, params: Option<&Json>, id: Option<Json>) -> Vec<Json> {
        let Some((uri, (line, ch))) = Self::doc_pos(params) else {
            return vec![self.respond_err(id, -32602, "invalid params")];
        };
        let Some((d, a)) = self.doc_analysis(&uri) else {
            return vec![self.respond(id, Json::Null)];
        };
        let idx = LineIndex::new(&d.text);
        let wl = idx.to_wlel(line, ch);
        match definition(a, &idx, wl) {
            Some(site) => {
                let (sl, sc) = idx.to_lsp(site.span.start);
                let (el, ec) = idx.to_lsp(site.span.end);
                let result = jobj(vec![
                    ("uri", jstr(file_to_uri(&site.file))),
                    ("range", range_json(sl, sc, el, ec)),
                ]);
                vec![self.respond(id, result)]
            }
            None => vec![self.respond(id, Json::Null)],
        }
    }

    fn completion_req(&mut self, params: Option<&Json>, id: Option<Json>) -> Vec<Json> {
        let Some((uri, (line, ch))) = Self::doc_pos(params) else {
            return vec![self.respond_err(id, -32602, "invalid params")];
        };
        let Some((d, a)) = self.doc_analysis(&uri) else {
            return vec![self.respond(id, jobj(vec![
                ("isIncomplete", Json::Bool(false)),
                ("items", jarr(vec![])),
            ]))];
        };
        let idx = LineIndex::new(&d.text);
        let wl = idx.to_wlel(line, ch);
        let items: Vec<Json> = completion(a, wl)
            .into_iter()
            .map(|c| {
                jobj(vec![
                    ("label", jstr(c.label)),
                    ("kind", jint(c.kind)),
                    ("detail", jstr(c.detail)),
                ])
            })
            .collect();
        let result =
            jobj(vec![("isIncomplete", Json::Bool(false)), ("items", jarr(items))]);
        vec![self.respond(id, result)]
    }
}

// ---------------------------------------------------------------------------
// stdio framing (Content-Length headers) + main loop
// ---------------------------------------------------------------------------

/// read one `Content-Length`-framed message; None on EOF or malformed frame
pub fn read_message(input: &mut impl BufRead) -> Option<String> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = input.read_line(&mut line).ok()?;
        if n == 0 {
            return None;
        }
        let t = line.trim_end_matches(['\r', '\n']);
        if t.is_empty() {
            break;
        }
        if let Some(v) = t.strip_prefix("Content-Length:") {
            len = v.trim().parse::<usize>().ok();
        }
        // Content-Type and unknown headers are ignored
    }
    let len = len?;
    let mut buf = vec![0u8; len];
    input.read_exact(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

pub fn write_message(output: &mut impl Write, body: &str) -> std::io::Result<()> {
    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    output.flush()
}

/// serve LSP over the given streams (stdin/stdout in production); returns
/// after the `exit` notification or EOF
pub fn run(mut input: impl BufRead, mut output: impl Write) {
    eprintln!("wlel lsp: language server listening on stdio");
    let mut srv = Server::new();
    while let Some(body) = read_message(&mut input) {
        match Json::parse(&body) {
            Ok(msg) => {
                for out in srv.handle(&msg) {
                    if write_message(&mut output, &out.to_string()).is_err() {
                        return;
                    }
                }
            }
            Err(e) => eprintln!("wlel lsp: malformed message: {e}"),
        }
        if srv.done {
            break;
        }
    }
}
