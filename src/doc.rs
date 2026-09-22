//! `wlel doc` — markdown documentation from `///` doc comments.
//!
//! A module page is generated per source file: parse the file (via the same
//! lexer/parser pipeline as the compiler), collect every block of consecutive
//! `///` comments, and attach each block to the top-level declaration that
//! immediately follows it (adjacent, or separated by at most one blank line).
//! A doc block at the top of the file that is *not* attached to a declaration
//! becomes the module description. Plain `//` comments are never docs.
//!
//! Output is plain markdown, one page per module plus an `index.md` — a
//! static site renderable by GitHub Pages, mkdocs, or any markdown viewer.
//! The embedded standard library gets its own page (from the same pipeline)
//! with an extra section for the compiler-builtin API that has no AST.

use crate::ast::*;
use crate::lexer::{Comment, Lexer};
use crate::parser::Parser;
use crate::stdsrc;

/// one generated documentation page
#[derive(Debug, Clone)]
pub struct ModuleDoc {
    /// module name (file stem, or "std")
    pub name: String,
    /// full markdown page (without the trailing newline)
    pub markdown: String,
    /// first line of the module description ("" when undocumented)
    pub summary: String,
    pub structs: usize,
    pub enums: usize,
    pub funcs: usize,
    pub methods: usize,
    /// true when the source declares `use std;` (the CLI then also emits the
    /// stdlib page — the embedded library is part of the program's surface)
    pub uses_std: bool,
}

impl ModuleDoc {
    /// "4 structs · 2 enums · 31 functions · 17 methods" (non-zero parts only)
    pub fn counts(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.structs > 0 {
            parts.push(format!("{} struct{}", self.structs, plural(self.structs)));
        }
        if self.enums > 0 {
            parts.push(format!("{} enum{}", self.enums, plural(self.enums)));
        }
        if self.funcs > 0 {
            parts.push(format!("{} function{}", self.funcs, plural(self.funcs)));
        }
        if self.methods > 0 {
            parts.push(format!("{} method{}", self.methods, plural(self.methods)));
        }
        if parts.is_empty() {
            "no public items".into()
        } else {
            parts.join(" · ")
        }
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// parse `src` and render its documentation page. Errors mirror `wlel fmt`:
/// a file that does not lex/parse is never documented.
pub fn document_source(src: &str, name: &str) -> Result<ModuleDoc, String> {
    let (tokens, comments) =
        Lexer::new(src).tokenize_with_comments().map_err(|e| e.to_string())?;
    let (program, errors) = Parser::new(&tokens).program();
    if !errors.is_empty() {
        return Err(errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"));
    }
    Ok(render(name, &program, comments))
}

/// documentation page for the embedded standard library, including the
/// compiler-builtin API that has no Wlel source (print, math, random, ...)
pub fn std_doc() -> ModuleDoc {
    let mut doc = document_source(stdsrc::STD_WL, "std").expect("std library documents");
    doc.markdown.push_str("\n\n");
    doc.markdown.push_str(BUILTIN_SECTION.trim());
    doc
}

// ---------------------------------------------------------------------------
// doc-comment extraction
// ---------------------------------------------------------------------------

/// one block of consecutive `///` lines: the trimmed text of each line plus
/// the 1-based source line of the block's last comment
struct DocBlock {
    last_line: usize,
    lines: Vec<String>,
}

fn is_doc_comment(c: &Comment) -> bool {
    // `/// text` — the comment text after `//` starts with one `/`; `//`
    // would be a four-slash comment, not a doc
    c.text.starts_with('/') && !c.text.starts_with("//")
}

/// group comments into doc blocks (consecutive source lines merge)
fn doc_blocks(comments: &[Comment]) -> Vec<DocBlock> {
    let mut blocks: Vec<DocBlock> = Vec::new();
    for c in comments {
        if !is_doc_comment(c) {
            continue;
        }
        let content = c.text[1..].strip_prefix(' ').unwrap_or(&c.text[1..]).trim_end().to_string();
        match blocks.last_mut() {
            Some(b) if c.line == b.last_line + 1 => {
                b.lines.push(content);
                b.last_line = c.line;
            }
            _ => blocks.push(DocBlock { last_line: c.line, lines: vec![content] }),
        }
    }
    blocks
}

/// gather the top-level declarations that can carry a doc block, in source
/// order; `tag` distinguishes impl-target entries so methods group under it
enum DocDecl<'a> {
    Struct(&'a StructDef),
    Enum(&'a EnumDef),
    Func(&'a FuncDef),
    /// one impl block — methods are extracted separately
    Impl(&'a ImplDef),
    /// a single method inside an impl (type_name = impl target)
    Method(&'a FuncDef, &'a str),
}

impl DocDecl<'_> {
    fn start_line(&self) -> usize {
        match self {
            DocDecl::Struct(s) => s.span.start.line,
            DocDecl::Enum(e) => e.span.start.line,
            DocDecl::Func(f) => f.span.start.line,
            DocDecl::Impl(i) => i.span.start.line,
            DocDecl::Method(m, _) => m.span.start.line,
        }
    }
}

/// attach doc blocks to declarations: a block belongs to the first
/// declaration at most one blank line below it (adjacent is the canonical
/// fmt output); unclaimed blocks above the first declaration become the
/// module description
fn attach<'a>(decls: &[DocDecl<'a>], blocks: Vec<DocBlock>) -> (Vec<Option<String>>, String) {
    let mut docs: Vec<Option<String>> = vec![None; decls.len()];
    let mut claimed = vec![false; blocks.len()];
    let mut module_doc = String::new();
    for (di, d) in decls.iter().enumerate() {
        // closest block above wins (a stray block two lines up must not beat
        // the doc directly above the declaration)
        let mut best: Option<usize> = None;
        for (bi, b) in blocks.iter().enumerate() {
            if claimed[bi] {
                continue;
            }
            let gap = d.start_line().saturating_sub(b.last_line);
            let closer =
                best.is_none_or(|prev| b.last_line > blocks[prev].last_line);
            if (1..=2).contains(&gap) && closer {
                best = Some(bi);
            }
        }
        if let Some(bi) = best {
            docs[di] = Some(blocks[bi].lines.join("\n"));
            claimed[bi] = true;
        }
    }
    // every unclaimed block that sits above the first declaration is a
    // module-description candidate (top-of-file intro)
    let first_decl = decls.iter().map(|d| d.start_line()).min().unwrap_or(usize::MAX);
    for (bi, b) in blocks.iter().enumerate() {
        if !claimed[bi] && b.last_line < first_decl {
            if !module_doc.is_empty() {
                module_doc.push_str("\n\n");
            }
            module_doc.push_str(&b.lines.join("\n"));
        }
    }
    (docs, module_doc)
}

/// all doc-carrying declarations of a program in source order (uses and test
/// blocks are not API and never carry docs)
fn collect_decls(program: &Program) -> Vec<DocDecl<'_>> {
    let mut decls: Vec<DocDecl> = Vec::new();
    for s in &program.structs {
        decls.push(DocDecl::Struct(s));
    }
    for e in &program.enums {
        decls.push(DocDecl::Enum(e));
    }
    for f in &program.funcs {
        decls.push(DocDecl::Func(f));
    }
    for i in &program.impls {
        decls.push(DocDecl::Impl(i));
        for m in &i.methods {
            decls.push(DocDecl::Method(m, &i.type_name));
        }
    }
    decls.sort_by_key(|d| d.start_line());
    decls
}

// ---------------------------------------------------------------------------
// markdown rendering
// ---------------------------------------------------------------------------

/// one method entry: the method plus its doc block
type MethodEntry<'a> = (&'a FuncDef, &'a Option<String>);
/// methods of one impl target, grouped by the type they extend
type MethodGroup<'a> = (&'a str, Vec<MethodEntry<'a>>);

fn render(name: &str, program: &Program, comments: Vec<Comment>) -> ModuleDoc {
    let blocks = doc_blocks(&comments);
    let decls = collect_decls(program);
    let (docs, module_desc) = attach(&decls, blocks);

    let mut out = String::new();
    out.push_str(&format!("# {name}\n\n"));
    if !module_desc.is_empty() {
        out.push_str(&module_desc);
        out.push_str("\n\n");
    }

    // partition in source order, keeping only documented items where it
    // matters (undocumented items still appear — the page shows the full API)
    let mut structs: Vec<(&StructDef, &Option<String>)> = Vec::new();
    let mut enums: Vec<(&EnumDef, &Option<String>)> = Vec::new();
    let mut funcs: Vec<(&FuncDef, &Option<String>)> = Vec::new();
    let mut methods: Vec<MethodGroup> = Vec::new();
    for (d, doc) in decls.iter().zip(&docs) {
        match d {
            DocDecl::Struct(s) => structs.push((s, doc)),
            DocDecl::Enum(e) => enums.push((e, doc)),
            DocDecl::Func(f) => funcs.push((f, doc)),
            DocDecl::Impl(_) => {}
            DocDecl::Method(m, ty) => match methods.last_mut() {
                Some((t, ms)) if *t == *ty => ms.push((m, doc)),
                _ => methods.push((ty, vec![(m, doc)])),
            },
        }
    }

    // contents line (only non-empty sections)
    let mut toc: Vec<&str> = Vec::new();
    if !structs.is_empty() {
        toc.push("[Structs](#structs)");
    }
    if !enums.is_empty() {
        toc.push("[Enums](#enums)");
    }
    if !funcs.is_empty() {
        toc.push("[Functions](#functions)");
    }
    if !methods.is_empty() {
        toc.push("[Methods](#methods)");
    }
    if !toc.is_empty() {
        out.push_str(&format!("**Contents:** {}\n\n", toc.join(" · ")));
    }

    if !structs.is_empty() {
        out.push_str("## Structs\n\n");
        for (s, doc) in &structs {
            out.push_str(&format!("### {}\n\n", type_heading(&s.name, &s.type_params)));
            out.push_str(&format!("```wl\n{}\n```\n\n", struct_def_text(s)));
            push_doc(&mut out, doc);
        }
    }
    if !enums.is_empty() {
        out.push_str("## Enums\n\n");
        for (e, doc) in &enums {
            out.push_str(&format!("### {}\n\n", type_heading(&e.name, &e.type_params)));
            out.push_str(&format!("```wl\n{}\n```\n\n", enum_def_text(e)));
            push_doc(&mut out, doc);
        }
    }
    if !funcs.is_empty() {
        out.push_str("## Functions\n\n");
        for (f, doc) in &funcs {
            out.push_str(&format!("### {}\n\n", f.name));
            out.push_str(&format!("```wl\n{}\n```\n\n", fn_sig(f)));
            push_doc(&mut out, doc);
        }
    }
    if !methods.is_empty() {
        out.push_str("## Methods\n\n");
        for (ty, ms) in &methods {
            // find the impl's own type parameters for the heading
            let params = program
                .impls
                .iter()
                .find(|i| i.type_name == *ty)
                .map(|i| i.type_params.as_slice())
                .unwrap_or(&[]);
            out.push_str(&format!("### {}\n\n", type_heading(ty, params)));
            for (m, doc) in ms {
                out.push_str(&format!("#### {}\n\n", m.name));
                out.push_str(&format!("```wl\n{}\n```\n\n", fn_sig(m)));
                push_doc(&mut out, doc);
            }
        }
    }

    let summary = module_desc.lines().next().unwrap_or("").to_string();
    let markdown = out.trim_end().to_string();
    ModuleDoc {
        name: name.into(),
        markdown,
        summary,
        structs: structs.len(),
        enums: enums.len(),
        funcs: funcs.len(),
        methods: methods.iter().map(|(_, ms)| ms.len()).sum(),
        uses_std: program.uses.iter().any(|u| u.path.is_none()),
    }
}

fn push_doc(out: &mut String, doc: &Option<String>) {
    if let Some(d) = doc {
        out.push_str(d);
        out.push_str("\n\n");
    }
}

fn type_heading(name: &str, params: &[String]) -> String {
    if params.is_empty() {
        name.into()
    } else {
        format!("{name}[{}]", params.join(", "))
    }
}

/// `fn name[T](p: T, q) -> ret` (or `extern fn ...;`)
fn fn_sig(f: &FuncDef) -> String {
    let mut head = format!("fn {}", f.name);
    if !f.type_params.is_empty() {
        head.push_str(&format!("[{}]", f.type_params.join(", ")));
    }
    let params = f
        .params
        .iter()
        .map(|p| match &p.ty {
            Some(t) => format!("{}: {}", p.name, t),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    head.push_str(&format!("({params})"));
    if let Some(r) = &f.ret_type {
        head.push_str(&format!(" -> {r}"));
    }
    if f.is_extern {
        format!("extern {head};")
    } else {
        head
    }
}

fn struct_def_text(s: &StructDef) -> String {
    let mut t = format!("struct {}", type_heading(&s.name, &s.type_params));
    if s.fields.is_empty() {
        t.push_str(" {}");
        return t;
    }
    t.push_str(" {\n");
    for (fname, fty) in &s.fields {
        t.push_str(&format!("    {fname}: {fty},\n"));
    }
    t.push('}');
    t
}

fn enum_def_text(e: &EnumDef) -> String {
    let mut t = format!("enum {}", type_heading(&e.name, &e.type_params));
    if e.variants.is_empty() {
        t.push_str(" {}");
        return t;
    }
    t.push_str(" {\n");
    for v in &e.variants {
        if v.payloads.is_empty() {
            t.push_str(&format!("    {},\n", v.name));
        } else {
            t.push_str(&format!("    {}({}),\n", v.name, v.payloads.join(", ")));
        }
    }
    t.push('}');
    t
}

/// compiler-builtin API documented by hand (these have no Wlel source, so
/// `///` extraction cannot reach them)
const BUILTIN_SECTION: &str = "
## Builtin functions

These are compiler builtins (checker/codegen level) — not written in Wlel,
so they do not appear in the sections above.

| API | Description |
|-----|-------------|
| `std::print_str(s)` / `std::println_str(s)` | write a string to stdout (with/without newline) |
| `std::print_int/println_int(v)` | write an integer |
| `std::print_float/println_float(v)` | write a float |
| `std::print_bool/println_bool(b)` | write a bool |
| `std::strlen(s) -> int`, `std::streq(a, b) -> bool` | string length / byte equality |
| `std::abs(x)`, `std::min(a, b)`, `std::max(a, b)` | small numeric helpers |
| `std::len(coll) -> int` | length of an array or `Vec` |
| `std::checked_add/sub/mul(a, b, &out) -> bool` | overflow-checked arithmetic (`__builtin_*_overflow`) |
| `std::format(fmt, ...) -> string` | mini-printf: `{}` placeholders accept int/float/bool/string |
| `std::parse_float(s, &out) -> bool` | strtod semantics; must consume the whole string |
| `std::float_to_str(f) -> string` | `%g`-style shortest rendering |
| `std::math::sqrt/cbrt/exp/log/log2/log10/sin/cos/tan/asin/acos/atan/sinh/cosh/tanh/floor/ceil/round/trunc/fabs(x)` | libm wrappers (one argument) |
| `std::math::pow/atan2/fmin/fmax/hypot/fmod(a, b)` | libm wrappers (two arguments) |
| `std::random::seed(s)`, `next() -> u64`, `int(n) -> int`, `float() -> float` | xorshift64*; `seed` makes the stream reproducible |
| `std::sort(coll, cmp)`, `std::binary_search(coll, needle, cmp)` | quicksort + binary search; `cmp(a, b) -> int` is a plain Wlel function; forms `(arr, cmp)`, `(ptr, n, cmp)`, `(vec, cmp)` |
| `std::fs::open(path, mode) -> Result[*File, string]` | open a file handle (Result-based) |
| `std::fs::read_all(f) -> Result[string, string]`, `fs::read(f, buf, n) -> Result[int, string]` | read (EOF = `Ok(0)`) |
| `std::fs::write_all(f, s) -> Result[int, string]`, `fs::write(f, buf, n) -> Result[int, string]` | write |
| `std::fs::close(f)` | close (idempotent, defer-friendly) |
| `sys::argc() -> int`, `sys::arg(i) -> string`, `sys::exit(code)` | process arguments / exit |
| `sys::read_file(path, &out) -> bool`, `sys::write_file(path, contents) -> bool` | whole-file raw I/O (bool + out-param) |
| `sys::mono_ms() -> int`, `sys::unix_ms() -> int` | monotonic / wall-clock milliseconds |
| `sys::thread(work, data) -> *Thread`, `sys::join(t)` | spawn/join a thread (fresh arena per thread) |
| `sys::mutex_new() -> *Mutex`, `lock/unlock/free` | mutexes |
| `sys::chan_new[T]() -> *Chan[T]`, `send(ch, v) -> bool`, `recv(ch, &out) -> bool`, `close(ch)`, `free(ch)` | unbounded FIFO channels |
| `sys::sleep_ms(ms)` | sleep the current thread |
";

// ---------------------------------------------------------------------------
// index page
// ---------------------------------------------------------------------------

/// the `index.md` listing every generated module page
pub fn index_markdown(title: &str, modules: &[ModuleDoc]) -> String {
    let mut out = format!("# {title}\n\nGenerated by `wlel doc` — {} module{}.\n\n", modules.len(), plural(modules.len()));
    out.push_str("| Module | Items | Description |\n|--------|-------|-------------|\n");
    for m in modules {
        let desc = if m.summary.is_empty() { "—".to_string() } else { m.summary.clone() };
        out.push_str(&format!(
            "| [{}]({}.md) | {} | {} |\n",
            m.name, m.name, m.counts(), desc
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_comment_shape() {
        let blocks = doc_blocks(&[
            Comment { line: 1, col: 1, text: "/ one".into() },
            Comment { line: 2, col: 1, text: "/ two".into() },
            Comment { line: 4, col: 1, text: "/ three".into() },
            Comment { line: 5, col: 1, text: " plain".into() },
            Comment { line: 6, col: 1, text: "// not a doc".into() },
        ]);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].lines, vec!["one", "two"]);
        assert_eq!(blocks[0].last_line, 2);
        assert_eq!(blocks[1].lines, vec!["three"]);
    }
}
