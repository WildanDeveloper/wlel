//! The self-hosted parser (`selfhost/parser.wlel`) must build the same tree as
//! the Rust parser for every `.wlel` file in the repository.
//!
//! Both sides print a canonical one-node-per-line dump; this file produces the
//! Rust side and compares. Literals are printed as their **source slice**, so
//! `10u8` and `1e-9` compare exactly instead of depending on float formatting.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use wlel::ast::*;
use wlel::lexer::Lexer;
use wlel::parser::Parser;
use wlel::span::Span;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if matches!(name.as_ref(), "target" | ".git" | "node_modules" | ".wlel") {
                    continue;
                }
                walk(&path, out);
            } else if name.ends_with(".wlel") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root(), &mut out);
    out.sort();
    out
}

// ---- source slices --------------------------------------------------------

struct Src<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Src<'a> {
    fn new(text: &'a str) -> Src<'a> {
        let mut starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Src { text, starts }
    }

    fn at(&self, line: usize, col: usize) -> usize {
        self.starts[line - 1] + col - 1
    }

    fn slice(&self, span: Span) -> &'a str {
        let a = self.at(span.start.line, span.start.col);
        let b = self.at(span.end.line, span.end.col);
        &self.text[a..b]
    }
}

// ---- dump -----------------------------------------------------------------

fn pad(b: &mut String, depth: usize) {
    for _ in 0..depth {
        b.push_str("  ");
    }
}

fn line(b: &mut String, depth: usize, tag: &str, text: &str) {
    pad(b, depth);
    if !tag.is_empty() {
        b.push_str(tag);
        b.push(':');
    }
    b.push_str(text);
    b.push('\n');
}

fn tparams(b: &mut String, p: &[String]) {
    if p.is_empty() {
        return;
    }
    b.push('[');
    b.push_str(&p.join(","));
    b.push(']');
}

fn dump_expr(b: &mut String, s: &Src, e: &Expr, depth: usize, tag: &str) {
    let mut head = String::new();
    match &e.node {
        ExprKind::Ident(n) => head.push_str(&format!("ident {n}")),
        // literals print their source spelling
        ExprKind::Int(_) | ExprKind::UInt(_) => {
            head.push_str(&format!("int {}", s.slice(e.span)))
        }
        ExprKind::Float(_) => head.push_str(&format!("float {}", s.slice(e.span))),
        ExprKind::Str(_) => head.push_str(&format!("str {}", s.slice(e.span))),
        ExprKind::Bool(v) => head.push_str(&format!("bool {}", if *v { 1 } else { 0 })),
        ExprKind::Unary(op, _) => {
            let n = match op {
                UnOp::Neg => 0,
                UnOp::Not => 1,
                UnOp::BitNot => 2,
            };
            head.push_str(&format!("unary {n}"));
        }
        ExprKind::AddrOf(_) => head.push_str("unary 3"),
        ExprKind::Deref(_) => head.push_str("unary 4"),
        ExprKind::Binary(op, _, _) => {
            let n = match op {
                BinOp::Or => 0,
                BinOp::And => 1,
                BinOp::BitOr => 2,
                BinOp::BitXor => 3,
                BinOp::BitAnd => 4,
                BinOp::Shl => 5,
                BinOp::Shr => 6,
                BinOp::Eq => 7,
                BinOp::Ne => 8,
                BinOp::Lt => 9,
                BinOp::Gt => 10,
                BinOp::Le => 11,
                BinOp::Ge => 12,
                BinOp::Add => 13,
                BinOp::Sub => 14,
                BinOp::Mul => 15,
                BinOp::Div => 16,
                BinOp::Mod => 17,
            };
            head.push_str(&format!("binary {n}"));
        }
        ExprKind::Call(name, ta, _) => {
            head.push_str(&format!("call {name}"));
            if !ta.is_empty() {
                head.push_str(&format!("[{}]", ta.join(", ")));
            }
        }
        ExprKind::CallExpr(_, _) => head.push_str("call-expr"),
        ExprKind::MethodCall(_, m, _) => head.push_str(&format!("method {m}")),
        ExprKind::Field(_, f) => head.push_str(&format!("field {f}")),
        ExprKind::Index(_, _) => head.push_str("index"),
        ExprKind::Cast(t, _) => head.push_str(&format!("cast {t}")),
        ExprKind::StructLit(n, _) => head.push_str(&format!("struct-lit {n}")),
        ExprKind::ArrayLit(_) => head.push_str("array"),
        ExprKind::Match(_) => head.push_str("match"),
        ExprKind::Try(_) => head.push_str("try"),
        ExprKind::New(t, _) => head.push_str(&format!("new {t}")),
        ExprKind::Sizeof(t) => head.push_str(&format!("sizeof {t}")),
        ExprKind::CurrentArena => head.push_str("current-arena"),
        ExprKind::EnumLit(..) => head.push_str("enum-lit"),
    }
    line(b, depth, tag, &head);
    match &e.node {
        ExprKind::Unary(_, x) | ExprKind::AddrOf(x) | ExprKind::Deref(x) | ExprKind::Cast(_, x)
        | ExprKind::Try(TryExpr { inner: x, .. }) => dump_expr(b, s, x, depth + 1, ""),
        ExprKind::Binary(_, l, r) => {
            dump_expr(b, s, l, depth + 1, "l");
            dump_expr(b, s, r, depth + 1, "r");
        }
        ExprKind::Call(_, _, args) => {
            for a in args {
                dump_expr(b, s, a, depth + 1, "arg");
            }
        }
        ExprKind::CallExpr(c, args) => {
            dump_expr(b, s, c, depth + 1, "callee");
            for a in args {
                dump_expr(b, s, a, depth + 1, "arg");
            }
        }
        ExprKind::MethodCall(r, _, args) => {
            dump_expr(b, s, r, depth + 1, "recv");
            for a in args {
                dump_expr(b, s, a, depth + 1, "arg");
            }
        }
        ExprKind::Field(o, _) => dump_expr(b, s, o, depth + 1, "recv"),
        ExprKind::Index(v, i) => {
            dump_expr(b, s, v, depth + 1, "value");
            dump_expr(b, s, i, depth + 1, "index");
        }
        ExprKind::StructLit(_, fields) => {
            for (n, v) in fields {
                line(b, depth + 1, "", &format!("init {n}"));
                dump_expr(b, s, v, depth + 2, "value");
            }
        }
        ExprKind::ArrayLit(items) => {
            for it in items {
                dump_expr(b, s, it, depth + 1, "elem");
            }
        }
        ExprKind::Match(m) => {
            dump_expr(b, s, &m.scrutinee, depth + 1, "scrutinee");
            for arm in &m.arms {
                line(b, depth + 1, "", "arm");
                dump_pattern(b, &arm.pattern, depth + 2, "pattern");
                match &arm.body {
                    MatchBody::Expr(x) => dump_expr(b, s, x, depth + 2, "body"),
                    MatchBody::Block(bl) => dump_block(b, s, bl, depth + 2, "body"),
                }
            }
        }
        ExprKind::New(_, Some(c)) => dump_expr(b, s, c, depth + 1, "count"),
        _ => {}
    }
}

fn dump_pattern(b: &mut String, pat: &Pattern, depth: usize, tag: &str) {
    match pat {
        Pattern::Wildcard => line(b, depth, tag, "wildcard"),
        Pattern::Variant { name, binds, .. } => {
            // the Rust AST cannot tell `Point` from `Point()`, and both mean
            // the same thing: a zero-payload variant
            if binds.is_empty() {
                line(b, depth, tag, &format!("ident {name}"));
            } else {
                let names: Vec<&str> = binds.iter().map(|b| b.name.as_str()).collect();
                line(b, depth, tag, &format!("variant-pat {name}({})", names.join(",")));
            }
        }
    }
}

fn dump_block(b: &mut String, s: &Src, bl: &Block, depth: usize, tag: &str) {
    line(b, depth, tag, "block");
    for st in &bl.0 {
        dump_stmt(b, s, st, depth + 1);
    }
}

fn dump_stmt(b: &mut String, s: &Src, st: &Stmt, depth: usize) {
    match &st.node {
        StmtKind::Let(name, ty, v) => {
            let mut head = format!("let {name}");
            if let Some(t) = ty {
                head.push(':');
                head.push_str(t);
            }
            line(b, depth, "", &head);
            dump_expr(b, s, v, depth + 1, "value");
        }
        StmtKind::Assign(a) => {
            let op = match a.op {
                CompoundOp::Set => 0,
                CompoundOp::Add => 1,
                CompoundOp::Sub => 2,
                CompoundOp::Mul => 3,
                CompoundOp::Div => 4,
                CompoundOp::Mod => 5,
            };
            line(b, depth, "", &format!("assign {op}"));
            dump_expr(b, s, &a.target, depth + 1, "target");
            dump_expr(b, s, &a.value, depth + 1, "value");
        }
        StmtKind::Return(v) => {
            line(b, depth, "", "return");
            if let Some(v) = v {
                dump_expr(b, s, v, depth + 1, "value");
            }
        }
        StmtKind::If(i) => dump_if(b, s, i, depth),
        StmtKind::While(c, body) => {
            line(b, depth, "", "while");
            dump_expr(b, s, c, depth + 1, "cond");
            dump_block(b, s, body, depth + 1, "body");
        }
        StmtKind::For(var, a, z, body) => {
            line(b, depth, "", &format!("for {var}"));
            dump_expr(b, s, a, depth + 1, "start");
            dump_expr(b, s, z, depth + 1, "end");
            dump_block(b, s, body, depth + 1, "body");
        }
        StmtKind::ForIn(f) => {
            line(b, depth, "", &format!("for-in {}", f.var));
            dump_expr(b, s, &f.iter, depth + 1, "iter");
            dump_block(b, s, &f.body, depth + 1, "body");
        }
        StmtKind::Break => line(b, depth, "", "break"),
        StmtKind::Continue => line(b, depth, "", "continue"),
        StmtKind::Defer(d) => {
            line(b, depth, "", "defer");
            match d {
                DeferBody::Expr(x) => dump_expr(b, s, x, depth + 1, "body"),
                DeferBody::Block(bl) => dump_block(b, s, bl, depth + 1, "body"),
            }
        }
        StmtKind::Arena(size, body) => {
            line(b, depth, "", "arena");
            if let Some(sz) = size {
                dump_expr(b, s, sz, depth + 1, "size");
            }
            dump_block(b, s, body, depth + 1, "body");
        }
        StmtKind::ExprStmt(x) => {
            line(b, depth, "", "expr");
            dump_expr(b, s, x, depth + 1, "");
        }
        StmtKind::Block(bl) => {
            line(b, depth, "", "stmt-block");
            dump_block(b, s, bl, depth + 1, "");
        }
        StmtKind::Match(m) => {
            // statement-form match: the same node the expression form uses
            line(b, depth, "", "match-stmt");
            dump_expr(b, s, &m.scrutinee, depth + 1, "scrutinee");
            for arm in &m.arms {
                line(b, depth + 1, "", "arm");
                dump_pattern(b, &arm.pattern, depth + 2, "pattern");
                match &arm.body {
                    MatchBody::Expr(x) => dump_expr(b, s, x, depth + 2, "body"),
                    MatchBody::Block(bl) => dump_block(b, s, bl, depth + 2, "body"),
                }
            }
        }
    }
}

fn dump_if(b: &mut String, s: &Src, i: &IfStmt, depth: usize) {
    line(b, depth, "", "if");
    dump_expr(b, s, &i.cond, depth + 1, "cond");
    dump_block(b, s, &i.then_body, depth + 1, "then");
    match &i.else_branch {
        Some(ElseBranch::Block(bl)) => dump_block(b, s, bl, depth + 1, "else"),
        Some(ElseBranch::If(inner)) => {
            pad(b, depth + 1);
            b.push_str("else-if:\n");
            dump_if(b, s, inner, depth + 1);
        }
        None => {}
    }
}

fn dump_fn(b: &mut String, s: &Src, f: &FuncDef, depth: usize) {
    let mut head = format!("fn {}", f.name);
    tparams(&mut head, &f.type_params);
    if f.is_extern {
        head.push_str(" extern");
    }
    line(b, depth, "", &head);
    for p in &f.params {
        let mut ph = format!("param {}", p.name);
        if let Some(t) = &p.ty {
            ph.push(':');
            ph.push_str(t);
        }
        // the list tag makes this `param:param ...`, matching the Wlel side
        line(b, depth + 1, "param", &ph);
    }
    line(b, depth + 1, "ret", f.ret_type.as_deref().unwrap_or("-"));
    dump_block(b, s, &f.body, depth + 1, "body");
}

fn dump_program(b: &mut String, s: &Src, p: &Program) {
    line(b, 0, "", "program");
    // source order: the AST keeps one vector per item kind
    let mut order: Vec<(usize, usize, usize)> = Vec::new(); // (line, kind, index)
    if !p.module.is_empty() {
        order.push((p.module_span.start.line, 0, 0));
    }
    for (i, u) in p.uses.iter().enumerate() {
        order.push((u.span.start.line, 1, i));
    }
    for (i, c) in p.consts.iter().enumerate() {
        order.push((c.span.start.line, 2, i));
    }
    for (i, st) in p.structs.iter().enumerate() {
        order.push((st.span.start.line, 3, i));
    }
    for (i, e) in p.enums.iter().enumerate() {
        order.push((e.span.start.line, 4, i));
    }
    for (i, im) in p.impls.iter().enumerate() {
        order.push((im.span.start.line, 5, i));
    }
    for (i, f) in p.funcs.iter().enumerate() {
        order.push((f.span.start.line, 6, i));
    }
    for (i, t) in p.tests.iter().enumerate() {
        order.push((t.span.start.line, 7, i));
    }
    order.sort();

    for (_, kind, i) in order {
        match kind {
            0 => line(b, 1, "", &format!("module {}", p.module)),
            1 => {
                let u = &p.uses[i];
                let mut head = format!("use {}", u.path.as_deref().unwrap_or("std"));
                if let Some(a) = &u.alias {
                    head.push_str(&format!(" as {a}"));
                }
                match &u.names {
                    ImportNames::None => {}
                    ImportNames::All => head.push_str(" { * }"),
                    ImportNames::Some(list) => {
                        head.push_str(" {");
                        for n in list {
                            head.push(' ');
                            head.push_str(&n.name);
                            if let Some(a) = &n.alias {
                                head.push_str(&format!(" as {a}"));
                            }
                        }
                        head.push_str(" }");
                    }
                }
                line(b, 1, "", &head);
            }
            2 => {
                let c = &p.consts[i];
                line(b, 1, "", &format!("const {}:{}", c.name, c.ty));
                dump_expr(b, s, &c.value, 2, "value");
            }
            3 => {
                let st = &p.structs[i];
                let mut head = format!("struct {}", st.name);
                tparams(&mut head, &st.type_params);
                line(b, 1, "", &head);
                for (n, t) in &st.fields {
                    line(b, 2, "", &format!("field {n}:{t}"));
                }
            }
            4 => {
                let e = &p.enums[i];
                let mut head = format!("enum {}", e.name);
                tparams(&mut head, &e.type_params);
                line(b, 1, "", &head);
                for v in &e.variants {
                    let mut vh = format!("variant {}", v.name);
                    if !v.payloads.is_empty() {
                        vh.push_str(&format!("({})", v.payloads.join(", ")));
                    }
                    line(b, 2, "", &vh);
                }
            }
            5 => {
                let im = &p.impls[i];
                let mut head = format!("impl {}", im.type_name);
                tparams(&mut head, &im.type_params);
                line(b, 1, "", &head);
                for m in &im.methods {
                    dump_fn(b, s, m, 2);
                }
            }
            6 => dump_fn(b, s, &p.funcs[i], 1),
            7 => {
                let t = &p.tests[i];
                line(b, 1, "", &format!("test {}", t.name));
                dump_block(b, s, &t.body, 2, "body");
            }
            _ => {}
        }
    }
}

fn rust_dump(src: &str) -> Result<String, String> {
    let toks = Lexer::new(src).tokenize().map_err(|e| e.to_string())?;
    let (program, errors) = Parser::new(&toks).program();
    if !errors.is_empty() {
        return Err(errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"));
    }
    let s = Src::new(src);
    let mut out = String::new();
    dump_program(&mut out, &s, &program);
    Ok(out)
}

// ---- tests ----------------------------------------------------------------

/// Build the self-hosted parser once for the whole test binary: building it
/// per file turned the parity gate into 41 compiler invocations.
fn parser_binary() -> Option<&'static Path> {
    static BINARY: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    BINARY
        .get_or_init(|| {
            if !have("cc") && !have("gcc") && !have("clang") {
                eprintln!("skipping: no C compiler on PATH");
                return None;
            }
            let out = std::env::temp_dir().join(format!(
                "wlel_selfhost_parser_{}",
                std::process::id()
            ));
            let _ = fs::remove_file(&out);
            let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
                .args(["build", "selfhost/parser.wlel", "-O2", "-o"])
                .arg(&out)
                .current_dir(root())
                .output()
                .expect("spawn wlel build");
            assert!(
                build.status.success(),
                "the self-hosted parser does not build:\n{}{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            );
            Some(out)
        })
        .as_deref()
}

fn wlel_dump(bin: &Path, file: &str) -> String {
    let run = Command::new(bin)
        .arg(file)
        .arg("dump")
        .output()
        .expect("run the self-hosted parser");
    assert!(
        run.status.success(),
        "the self-hosted parser rejected {file}:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn first_difference(wlel: &str, rust: &str) -> String {
    let a: Vec<&str> = wlel.lines().collect();
    let b: Vec<&str> = rust.lines().collect();
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("line {i}:\n  wlel: {x}\n  rust: {y}");
        }
    }
    if a.len() != b.len() {
        return format!(
            "line counts differ: wlel {}, rust {}\n  wlel tail: {:?}\n  rust tail: {:?}",
            a.len(),
            b.len(),
            a.get(a.len().saturating_sub(2)),
            b.get(b.len().saturating_sub(2))
        );
    }
    "identical".into()
}

#[test]
fn the_self_hosted_parser_matches_the_rust_one() {
    if !have("cc") && !have("gcc") && !have("clang") {
        eprintln!("skipping: no C compiler on PATH");
        return;
    }
    let Some(bin) = parser_binary() else { return };
    let files = sources();
    let mut failures = Vec::new();
    for file in &files {
        let src = fs::read_to_string(file).expect("read");
        let expected = match rust_dump(&src) {
            Ok(d) => d,
            Err(_) => continue, // files the Rust parser itself rejects
        };
        let actual = wlel_dump(bin, file.to_str().unwrap());
        if actual != expected {
            failures.push(format!("{}: {}", file.display(), first_difference(&actual, &expected)));
        }
    }
    assert!(
        failures.is_empty(),
        "the self-hosted parser disagrees on {} of {} files:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn the_self_hosted_parser_reports_throughput() {
    if !have("cc") && !have("gcc") && !have("clang") {
        eprintln!("skipping: no C compiler on PATH");
        return;
    }
    let Some(bin) = parser_binary() else { return };
    let run = Command::new(bin)
        .args([root().join("flagship/json.wlel").to_str().unwrap(), "bench", "20"])
        .output()
        .expect("run");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let text = String::from_utf8_lossy(&run.stdout);
    assert!(text.contains("20 passes"), "{text}");
    let mbps: u64 = text.rsplit(' ').find_map(|w| w.parse().ok()).unwrap_or(0);
    assert!(mbps > 3, "parser throughput regressed: {text}");
}
