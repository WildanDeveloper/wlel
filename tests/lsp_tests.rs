//! LSP tests: hand-rolled JSON, position mapping, diagnostics, hover,
//! go-to-definition, completion, and the stdio framing loop — all exercised
//! through the same IO-free `Server::handle` the production loop uses.

use std::io::Cursor;
use wlel::lsp::{self, jarr, jint, jobj, jstr, Json, LineIndex, Server};
use wlel::span::Pos;

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// 1-based byte position of the first occurrence of `needle`
fn pos_of(text: &str, needle: &str) -> Pos {
    let off = text.find(needle).expect("needle not found in source");
    let line = text[..off].matches('\n').count() + 1;
    let line_start = text[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
    Pos { line, col: off - line_start + 1 }
}

/// position `off` bytes into `needle` (single-line needles) — lets a test
/// put the cursor on the interesting part (e.g. the `x` of `p.x`)
fn pos_at(text: &str, needle: &str, off: usize) -> Pos {
    let p = pos_of(text, needle);
    Pos { line: p.line, col: p.col + off }
}

fn analyze(text: &str) -> lsp::Analysis {
    lsp::analyze(text, None)
}

fn hover_at(a: &lsp::Analysis, text: &str, needle: &str) -> String {
    hover_off(a, text, needle, 0)
}

fn hover_off(a: &lsp::Analysis, text: &str, needle: &str, off: usize) -> String {
    let idx = LineIndex::new(text);
    let (md, _) = lsp::hover(a, &idx, pos_at(text, needle, off)).expect("no hover");
    md
}

fn def_at(a: &lsp::Analysis, text: &str, needle: &str) -> lsp::DefSite {
    def_off(a, text, needle, 0)
}

fn def_off(a: &lsp::Analysis, text: &str, needle: &str, off: usize) -> lsp::DefSite {
    let idx = LineIndex::new(text);
    lsp::definition(a, &idx, pos_at(text, needle, off)).expect("no definition")
}

fn request(id: i64, method: &str, params: Json) -> Json {
    jobj(vec![
        ("jsonrpc", jstr("2.0")),
        ("id", jint(id)),
        ("method", jstr(method)),
        ("params", params),
    ])
}

fn notification(method: &str, params: Json) -> Json {
    jobj(vec![("jsonrpc", jstr("2.0")), ("method", jstr(method)), ("params", params)])
}

fn text_document(uri: &str, text: &str, version: i64) -> Json {
    jobj(vec![
        ("uri", jstr(uri)),
        ("languageId", jstr("wlel")),
        ("version", jint(version)),
        ("text", jstr(text)),
    ])
}

fn open_doc(srv: &mut Server, uri: &str, text: &str) -> Vec<Json> {
    srv.handle(&notification(
        "textDocument/didOpen",
        jobj(vec![("textDocument", text_document(uri, text, 1))]),
    ))
}

/// diagnostics array of the first publishDiagnostics message
fn published_diags(msgs: &[Json]) -> Vec<Json> {
    msgs.iter()
        .find(|m| m.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics"))
        .and_then(|m| m.ptr(&["params", "diagnostics"]).cloned())
        .and_then(|d| d.as_arr().map(|a| a.to_vec()))
        .unwrap_or_default()
}

fn temp_wl_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wlel_lsp_{}_{}", tag, std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

#[test]
fn json_parse_serialize_roundtrip() {
    let src = r#"{"a":[1,2.5,-3],"b":"x\né🎉","c":{"d":true,"e":null},"f":"😀"}"#;
    let v = Json::parse(src).expect("parse");
    let s = v.to_string();
    let v2 = Json::parse(&s).expect("reparse");
    assert_eq!(v, v2);
    // surrogate pair 😀 (U+1F600) survives
    assert_eq!(v.ptr(&["b"]).and_then(|b| b.as_str()), Some("x\né🎉"));
    assert_eq!(v.ptr(&["f"]).and_then(|f| f.as_str()), Some("😀"));
    // integer precision kept
    assert_eq!(Json::parse("9007199254740993").unwrap(), Json::Int(9007199254740993));
}

#[test]
fn json_rejects_garbage() {
    assert!(Json::parse("{").is_err());
    assert!(Json::parse("[1,]").is_err());
    assert!(Json::parse("{\"a\":1} trailing").is_err());
    assert!(Json::parse("\"unterminated").is_err());
}

// ---------------------------------------------------------------------------
// position mapping (UTF-16)
// ---------------------------------------------------------------------------

#[test]
fn line_index_utf16_conversion() {
    // é = 2 bytes / 1 UTF-16 unit; 🎉 = 4 bytes / 2 UTF-16 units
    let text = "fn main() {\n    s := \"héllo🎉\";\n    sys::exit(0);\n}\n";
    let idx = LineIndex::new(text);
    // emoji starts at byte col 17, UTF-16 offset 15
    let (l, c) = idx.to_lsp(Pos { line: 2, col: 17 });
    assert_eq!((l, c), (1, 15));
    // and back
    let p = idx.to_wlel(1, 15);
    assert_eq!((p.line, p.col), (2, 17));
    // mid-surrogate clamps to the end of the character (UTF-16 offset 16 is
    // the low half of the emoji; the next boundary is byte col 21)
    let p = idx.to_wlel(1, 16);
    assert_eq!((p.line, p.col), (2, 21));
    // ascii identity
    let p = idx.to_wlel(2, 4);
    assert_eq!((p.line, p.col), (3, 5));
}

#[test]
fn line_index_word_at() {
    let text = "fn main() {\n    total := 5;\n}\n";
    let idx = LineIndex::new(text);
    let (word, span) = idx.word_at(pos_of(text, "total")).unwrap();
    assert_eq!(word, "total");
    assert_eq!((span.start.line, span.start.col), (2, 5));
    // cursor one past the end of the word still finds it
    let (word, _) = idx.word_at(Pos { line: 2, col: 10 }).unwrap();
    assert_eq!(word, "total");
    assert!(idx.word_at(Pos { line: 1, col: 11 }).is_none()); // on `{`
}

// ---------------------------------------------------------------------------
// diagnostics
// ---------------------------------------------------------------------------

#[test]
fn diagnostics_clean_program() {
    let text = "fn main() {\n    sys::exit(0);\n}\n";
    let mut srv = Server::new();
    let msgs = open_doc(&mut srv, "file:///clean.wl", text);
    let diags = published_diags(&msgs);
    assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
}

#[test]
fn diagnostics_parse_error_position() {
    let text = "fn main() {\n    x := ;\n}\n";
    let mut srv = Server::new();
    let msgs = open_doc(&mut srv, "file:///bad.wl", text);
    let diags = published_diags(&msgs);
    assert!(!diags.is_empty());
    let d = &diags[0];
    assert_eq!(d.get("severity").and_then(|s| s.as_i64()), Some(1));
    // 0-based LSP line 1 (the `x := ;` line)
    let line = d.ptr(&["range", "start", "line"]).and_then(|l| l.as_i64());
    assert_eq!(line, Some(1));
    assert!(d.get("message").and_then(|m| m.as_str()).unwrap().contains("syntax error"));
}

#[test]
fn diagnostics_check_error() {
    let text = "fn main() {\n    x := 5;\n    y := x + \"s\";\n    sys::exit(0);\n}\n";
    let mut srv = Server::new();
    let msgs = open_doc(&mut srv, "file:///check_err.wl", text);
    let diags = published_diags(&msgs);
    assert!(!diags.is_empty());
    assert_eq!(diags[0].get("severity").and_then(|s| s.as_i64()), Some(1));
    // errors are attributed to `wlel` as source
    assert_eq!(diags[0].get("source").and_then(|s| s.as_str()), Some("wlel"));
}

#[test]
fn diagnostics_warning_unused_var() {
    let text = "fn main() {\n    x := 5;\n    sys::exit(0);\n}\n";
    let mut srv = Server::new();
    let msgs = open_doc(&mut srv, "file:///warn.wl", text);
    let diags = published_diags(&msgs);
    assert!(!diags.is_empty());
    assert_eq!(diags[0].get("severity").and_then(|s| s.as_i64()), Some(2));
    assert!(diags[0].get("message").and_then(|m| m.as_str()).unwrap().contains("unused"));
}

#[test]
fn diagnostics_update_on_change() {
    let bad = "fn main() {\n    x := ;\n}\n";
    let good = "fn main() {\n    sys::exit(0);\n}\n";
    let mut srv = Server::new();
    open_doc(&mut srv, "file:///ch.wl", bad);
    let msgs = srv.handle(&notification(
        "textDocument/didChange",
        jobj(vec![
            ("textDocument", jobj(vec![("uri", jstr("file:///ch.wl")), ("version", jint(2))])),
            (
                "contentChanges",
                jarr(vec![jobj(vec![("text", jstr(good))])]),
            ),
        ]),
    ));
    let diags = published_diags(&msgs);
    assert!(diags.is_empty(), "fixed file must have no diagnostics");
    let version = msgs
        .iter()
        .find(|m| m.get("method").is_some())
        .and_then(|m| m.ptr(&["params", "version"]))
        .and_then(|v| v.as_i64());
    assert_eq!(version, Some(2));
}

#[test]
fn diagnostics_unresolved_import() {
    let dir = temp_wl_dir("imp");
    let text = "use \"does_not_exist.wl\";\nfn main() {\n    sys::exit(0);\n}\n";
    let path = dir.join("main.wl");
    std::fs::write(&path, text).unwrap();
    let uri = format!("file://{}", path.display());
    let mut srv = Server::new();
    let msgs = open_doc(&mut srv, &uri, text);
    let diags = published_diags(&msgs);
    assert!(!diags.is_empty());
    assert!(diags[0].get("message").and_then(|m| m.as_str()).unwrap().contains("import"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unsaved_buffer_import_reported() {
    let a = analyze("use \"x.wl\";\nfn main() { sys::exit(0); }\n");
    assert!(a
        .diagnostics
        .iter()
        .any(|d| d.severity == 1 && d.msg.contains("not saved")));
}

// ---------------------------------------------------------------------------
// hover
// ---------------------------------------------------------------------------

#[test]
fn hover_local_variable() {
    let text = "fn main() {\n    total := add(1, 2);\n    sys::exit(total);\n}\n\nfn add(a: int, b: int) -> int {\n    return a + b;\n}\n";
    let a = analyze(text);
    let md = hover_at(&a, text, "total");
    assert!(md.contains("total: int"), "{md}");
    // the call site shows the signature
    let md = hover_at(&a, text, "add(1");
    assert!(md.contains("fn add(a: int, b: int) -> int"), "{md}");
    assert!(md.contains("function"));
}

#[test]
fn hover_param_and_expression() {
    let text = "fn add(a: int, b: int) -> int {\n    return a + b;\n}\n";
    let a = analyze(text);
    let md = hover_at(&a, text, "a +");
    assert!(md.contains("a: int"), "{md}");
    // hovering the whole expression falls back to its type
    let idx = LineIndex::new(text);
    let (_, span) = lsp::hover(&a, &idx, pos_of(text, "a + b")).unwrap();
    // innermost wins: `a` is smaller than the binary expression
    assert_eq!(span.end.col - span.start.col, 1);
}

#[test]
fn hover_struct_field_literal() {
    let text = "struct Pt {\n    x: float,\n    y: float\n}\n\nfn main() {\n    p := Pt { x: 1.0, y: 2.0 };\n    w := p.x;\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    // struct literal name
    let md = hover_at(&a, text, "Pt { x: 1.0");
    assert!(md.contains("struct Pt { x: float, y: float }"), "{md}");
    // field access shows the field's type (cursor on the `x` of `p.x`)
    let md = hover_off(&a, text, "p.x;", 2);
    assert!(md.contains("x: float"), "{md}");
    assert!(md.contains("field of Pt"));
}

#[test]
fn hover_method() {
    let text = "struct Pt {\n    x: float,\n    y: float\n}\n\nimpl Pt {\n    fn len(self) -> float {\n        return self.x + self.y;\n    }\n}\n\nfn main() {\n    p := Pt { x: 1.0, y: 2.0 };\n    l := p.len();\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    let md = hover_off(&a, text, "p.len()", 2);
    assert!(md.contains("fn len(self) -> float"), "{md}");
    assert!(md.contains("method of Pt"));
}

#[test]
fn hover_std_function() {
    let text = "use std;\n\nfn main() {\n    v := vec_new[int]();\n    v.push(1);\n    sys::exit(v.len() as int);\n}\n";
    let a = analyze(text);
    let md = hover_at(&a, text, "vec_new");
    assert!(md.contains("fn vec_new[T]() -> Vec[T]"), "{md}");
}

#[test]
fn hover_generic_instantiation() {
    let text = "fn id[T](x: T) -> T {\n    return x;\n}\n\nfn main() {\n    y := id[int](5);\n    sys::exit(y);\n}\n";
    let a = analyze(text);
    let md = hover_at(&a, text, "id[int]");
    assert!(md.contains("fn id[T](x: T) -> T"), "{md}");
}

#[test]
fn hover_enum_and_variant() {
    let text = "enum Shape {\n    Circle(float),\n    Point\n}\n\nfn area(s: Shape) -> float {\n    return match s {\n        Circle(r) => r,\n        Point => 0.0\n    };\n}\n\nfn main() {\n    a := area(Circle(2.0));\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    let md = hover_at(&a, text, "Circle(r)");
    assert!(md.contains("Circle(float)"), "{md}");
    assert!(md.contains("variant of Shape"));
    // pattern binding type via the checker
    let md = hover_at(&a, text, "r) =>");
    assert!(md.contains("r: float"), "{md}");
}

#[test]
fn hover_type_annotation_and_primitive() {
    let text = "fn main() {\n    let x: int = 5;\n    sys::exit(x);\n}\n";
    let a = analyze(text);
    let md = hover_at(&a, text, "x: int");
    assert!(md.contains("x: int"), "{md}");
    // the annotation word itself
    let idx = LineIndex::new(text);
    let (word, _) = idx.word_at(pos_at(text, " int =", 1)).unwrap();
    assert_eq!(word, "int");
}

#[test]
fn hover_nothing_to_say() {
    let text = "fn main() {\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    let idx = LineIndex::new(text);
    // between tokens / on keywords: no hover
    assert!(lsp::hover(&a, &idx, Pos { line: 1, col: 3 }).is_none());
}

// ---------------------------------------------------------------------------
// go-to-definition
// ---------------------------------------------------------------------------

#[test]
fn definition_local_variable() {
    let text = "fn main() {\n    x := 5;\n    y := x + 1;\n    sys::exit(y);\n}\n";
    let a = analyze(text);
    let d = def_at(&a, text, "x + 1");
    assert_eq!(d.file, "<buffer>");
    // jumps back to the `x` of `x := 5` on line 2
    assert_eq!((d.span.start.line, d.span.start.col), (2, 5));
    assert_eq!(d.span.end.col - d.span.start.col, 1);
}

#[test]
fn definition_function_and_struct() {
    let text = "struct Pt {\n    x: float,\n    y: float\n}\n\nfn zero() -> Pt {\n    return Pt { x: 0.0, y: 0.0 };\n}\n\nfn main() {\n    p := zero();\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    let d = def_at(&a, text, "zero()");
    assert_eq!((d.span.start.line, d.span.start.col), (6, 4)); // `fn zero`
    let d = def_at(&a, text, "Pt { x: 0.0");
    assert_eq!((d.span.start.line, d.span.start.col), (1, 8)); // `struct Pt`
    // cursor on the type name in the declaration itself resolves too
    let d = def_off(&a, text, "struct Pt", 7);
    assert_eq!((d.span.start.line, d.span.start.col), (1, 8));
}

#[test]
fn definition_field_and_method() {
    let text = "struct Pt {\n    x: float,\n    y: float\n}\n\nimpl Pt {\n    fn len(self) -> float {\n        return self.x + self.y;\n    }\n}\n\nfn main() {\n    p := Pt { x: 1.0, y: 2.0 };\n    l := p.len();\n    w := p.x;\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    // field: lands on the field declaration line
    let d = def_off(&a, text, "p.x;", 2);
    assert_eq!(d.span.start.line, 2, "field x declared on line 2");
    // method: lands on the method name inside the impl block
    let d = def_off(&a, text, "p.len()", 2);
    assert_eq!(d.span.start.line, 7, "method len declared on line 7");
}

#[test]
fn definition_enum_variant() {
    let text = "enum Shape {\n    Circle(float),\n    Point\n}\n\nfn main() {\n    s := Circle(1.0);\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    let d = def_at(&a, text, "Circle(1.0");
    assert_eq!(d.span.start.line, 2, "variant Circle declared on line 2");
    // usage of a pattern binding jumps back to the binding in the pattern
    let text2 = "enum Opt {\n    Some(int),\n    None\n}\n\nfn f(o: Opt) -> int {\n    return match o {\n        Some(v) => v,\n        None => 0\n    };\n}\n\nfn main() {\n    sys::exit(f(Opt::None));\n}\n";
    let a2 = analyze(text2);
    let d = def_off(&a2, text2, "Some(v) => v,", 12);
    assert_eq!(d.span.start.line, 8, "binding v declared in the arm pattern");
}

#[test]
fn definition_shadowing_picks_nearest() {
    let text = "fn main() {\n    x := 1;\n    if true {\n        x := 2;\n        sys::exit(x);\n    }\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    let d = def_at(&a, text, "x);");
    assert_eq!((d.span.start.line, d.span.start.col), (4, 9));
}

#[test]
fn definition_out_of_scope_not_resolved() {
    let text = "fn main() {\n    if true {\n        x := 1;\n    }\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    // `x` is defined inside the if-block only; hovering later finds nothing
    let idx = LineIndex::new(text);
    assert!(lsp::definition(&a, &idx, pos_of(text, "exit(0")).is_none());
}

#[test]
fn definition_across_import() {
    let dir = temp_wl_dir("depx");
    let lib = "struct Gadget {\n    id: int\n}\n";
    let main = "use \"lib.wl\";\n\nfn main() {\n    g := Gadget { id: 1 };\n    sys::exit(g.id);\n}\n";
    let lib_path = dir.join("lib.wl");
    let main_path = dir.join("main.wl");
    std::fs::write(&lib_path, lib).unwrap();
    std::fs::write(&main_path, main).unwrap();

    let a = lsp::analyze(main, Some(&main_path));
    assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
    let d = def_at(&a, main, "Gadget {");
    assert!(d.file.ends_with("lib.wl"), "{}", d.file);
    assert_eq!(d.span.start.line, 1);

    // `use "lib.wl"` itself jumps to the file
    let idx = LineIndex::new(main);
    let d = lsp::definition(&a, &idx, pos_of(main, "lib.wl")).unwrap();
    assert!(d.file.ends_with("lib.wl"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// completion
// ---------------------------------------------------------------------------

#[test]
fn completion_locals_keywords_and_types() {
    let text = "fn main() {\n    total := 5;\n    sys::exit(t);\n}\n";
    let a = analyze(text);
    // cursor right after the incomplete identifier `t` (line 3)
    let pos = pos_at(text, "exit(t", 6);
    let items = lsp::completion(&a, pos);
    let find = |label: &str| items.iter().find(|c| c.label == label);
    let total = find("total").expect("local visible");
    assert_eq!(total.kind, 6);
    assert_eq!(total.detail, "int");
    // prefix filter is the client's job: everything is offered
    assert!(find("true").is_some(), "keyword offered");
    assert!(find("int").is_some(), "primitive offered");
    assert!(find("fn").is_some(), "keyword fn offered");
}

#[test]
fn completion_top_level_and_std() {
    let text = "use std;\n\nstruct Pt { x: float }\n\nfn main() {\n    v := vec_new[int]();\n    \n}\n";
    let a = analyze(text);
    let pos = Pos { line: 7, col: 5 };
    let items = lsp::completion(&a, pos);
    let find = |label: &str| items.iter().find(|c| c.label == label);
    // std function (spliced)
    let vn = find("vec_new").expect("std fn");
    assert_eq!(vn.kind, 3);
    assert!(vn.detail.contains("fn vec_new[T]() -> Vec[T]"));
    // user struct
    let pt = find("Pt").expect("struct");
    assert_eq!(pt.kind, 22);
    // std struct + enum + variants
    assert!(find("Vec").is_some());
    assert!(find("Result").is_some());
    assert!(find("Ok").is_some());
    assert!(find("Some").is_some());
    // method of Vec via impl
    let push = find("push").expect("method");
    assert_eq!(push.kind, 2);
    assert!(push.detail.contains("method of Vec"));
    // local
    assert!(find("v").is_some());
}

#[test]
fn completion_shadowing_last_wins() {
    let text = "fn main() {\n    x := 1;\n    {\n        x := \"s\";\n        \n    }\n    sys::exit(0);\n}\n";
    let a = analyze(text);
    let items = lsp::completion(&a, Pos { line: 5, col: 9 });
    let x = items.iter().find(|c| c.label == "x").expect("x");
    assert_eq!(x.detail, "string");
}

// ---------------------------------------------------------------------------
// protocol / server plumbing
// ---------------------------------------------------------------------------

#[test]
fn initialize_reports_capabilities() {
    let mut srv = Server::new();
    let msgs = srv.handle(&request(1, "initialize", jobj(vec![("capabilities", jobj(vec![]))])));
    assert_eq!(msgs.len(), 1);
    let caps = msgs[0].ptr(&["result", "capabilities"]).expect("capabilities");
    assert_eq!(caps.ptr(&["textDocumentSync", "change"]).and_then(|c| c.as_i64()), Some(1));
    assert_eq!(caps.get("hoverProvider").cloned(), Some(Json::Bool(true)));
    assert!(caps.get("definitionProvider").is_some());
    assert!(caps.get("completionProvider").is_some());
    assert_eq!(
        msgs[0].ptr(&["result", "serverInfo", "name"]).and_then(|n| n.as_str()),
        Some("wlel")
    );
}

#[test]
fn unknown_request_is_method_not_found() {
    let mut srv = Server::new();
    let msgs = srv.handle(&request(7, "workspace/executeCommand", jobj(vec![])));
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].ptr(&["error", "code"]).and_then(|c| c.as_i64()), Some(-32601));
    // unknown notifications are silent
    let msgs = srv.handle(&notification("workspace/didChangeConfiguration", jobj(vec![])));
    assert!(msgs.is_empty());
}

#[test]
fn shutdown_then_exit_stops_server() {
    let mut srv = Server::new();
    let msgs = srv.handle(&request(9, "shutdown", Json::Null));
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].get("result"), Some(&Json::Null));
    srv.handle(&notification("exit", jobj(vec![])));
    assert!(srv.done);
}

#[test]
fn did_close_publishes_empty() {
    let mut srv = Server::new();
    open_doc(&mut srv, "file:///c.wl", "fn main() { sys::exit(0); }\n");
    let msgs = srv.handle(&notification(
        "textDocument/didClose",
        jobj(vec![("textDocument", jobj(vec![("uri", jstr("file:///c.wl"))]))]),
    ));
    let diags = published_diags(&msgs);
    assert!(diags.is_empty());
    // the document is gone: a hover now returns null instead of analysis
    let msgs = srv.handle(&request(
        5,
        "textDocument/hover",
        jobj(vec![
            ("textDocument", jobj(vec![("uri", jstr("file:///c.wl"))])),
            ("position", jobj(vec![("line", jint(0)), ("character", jint(0))])),
        ]),
    ));
    assert_eq!(msgs[0].get("result"), Some(&Json::Null));
}

#[test]
fn hover_over_protocol_end_to_end() {
    let text = "fn main() {\n    x := 5;\n    sys::exit(x);\n}\n";
    let mut srv = Server::new();
    open_doc(&mut srv, "file:///h.wl", text);
    let msgs = srv.handle(&request(
        42,
        "textDocument/hover",
        jobj(vec![
            ("textDocument", jobj(vec![("uri", jstr("file:///h.wl"))])),
            // cursor on `x` of `sys::exit(x)` (line index 2, char 14)
            ("position", jobj(vec![("line", jint(2)), ("character", jint(14))])),
        ]),
    ));
    assert_eq!(msgs.len(), 1);
    let md = msgs[0]
        .ptr(&["result", "contents", "value"])
        .and_then(|v| v.as_str())
        .expect("hover contents");
    assert!(md.contains("x: int"), "{md}");
}

#[test]
fn stdio_framing_loop() {
    let text = "fn main() {\n    x := 5;\n    sys::exit(x);\n}\n";
    let frames = [
        request(1, "initialize", jobj(vec![("capabilities", jobj(vec![]))])),
        notification(
            "textDocument/didOpen",
            jobj(vec![("textDocument", text_document("file:///f.wl", text, 1))]),
        ),
        request(
            2,
            "textDocument/hover",
            jobj(vec![
                ("textDocument", jobj(vec![("uri", jstr("file:///f.wl"))])),
                ("position", jobj(vec![("line", jint(2)), ("character", jint(14))])),
            ]),
        ),
        request(
            3,
            "textDocument/definition",
            jobj(vec![
                ("textDocument", jobj(vec![("uri", jstr("file:///f.wl"))])),
                ("position", jobj(vec![("line", jint(2)), ("character", jint(14))])),
            ]),
        ),
        request(4, "shutdown", Json::Null),
        notification("exit", jobj(vec![])),
    ];
    let mut input = String::new();
    for f in &frames {
        let body = f.to_string();
        input.push_str(&format!("Content-Length: {}\r\n\r\n{}", body.len(), body));
    }
    let mut out: Vec<u8> = Vec::new();
    lsp::run(Cursor::new(input), &mut out);
    let out = String::from_utf8(out).unwrap();
    // every response id present, in order, plus diagnostics notification
    assert!(out.contains("\"id\":1"));
    assert!(out.contains("publishDiagnostics"));
    assert!(out.contains("\"id\":2"));
    assert!(out.contains("x: int"));
    assert!(out.contains("\"id\":3"));
    assert!(out.contains("file:///f.wl"));
    assert!(out.contains("\"id\":4"));
    assert!(out.contains("\"result\":null"));
    // framing is parseable end-to-end
    let mut cursor = Cursor::new(out.clone().into_bytes());
    let mut count = 0;
    while let Some(body) = lsp::read_message(&mut cursor) {
        Json::parse(&body).expect("valid json frame");
        count += 1;
    }
    assert_eq!(count, 5);
}

#[test]
fn uri_roundtrip() {
    let dir = temp_wl_dir("uri");
    let path = dir.join("a b.wl");
    let path_str = format!("{}", path.display()).replace(' ', "%20");
    let uri = format!("file://{path_str}");
    let got = lsp::uri_to_path(&uri).unwrap();
    assert_eq!(got, path);
    let _ = std::fs::remove_dir_all(&dir);
}

