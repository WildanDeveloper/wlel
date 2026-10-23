//! Regression tests for bugs found in the full-source audit: parser
//! recovery, checker validation, codegen for try/arrays/sort, stdlib
//! hardening, the LSP JSON reader and the CLI output paths.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use wlel::checker::Checker;
use wlel::codegen::{gen_program, gen_program_safe};
use wlel::lexer::Lexer;
use wlel::parser::Parser;

fn parse(src: &str) -> wlel::ast::Program {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
    p
}

fn check_err(src: &str) -> String {
    let mut p = parse(src);
    if p.uses.iter().any(|u| u.path.is_none()) {
        wlel::stdsrc::splice_std(&mut p);
    }
    Checker::check(&mut p).expect_err("must be rejected").msg
}

fn gen_src(src: &str, safe: bool) -> String {
    let mut p = parse(src);
    if p.uses.iter().any(|u| u.path.is_none()) {
        wlel::stdsrc::splice_std(&mut p);
    }
    Checker::check(&mut p).expect("typecheck");
    if safe {
        gen_program_safe(&p)
    } else {
        gen_program(&p)
    }
}

struct Case {
    dir: PathBuf,
}

impl Case {
    fn new(name: &str, src: &str) -> Case {
        let dir = std::env::temp_dir().join(format!(
            "wlel_reg_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::write(dir.join(format!("{name}.wlel")), src).expect("write");
        Case { dir }
    }

    fn run_cli(&self, args: &[&str]) -> (String, String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(args)
            .current_dir(&self.dir)
            .output()
            .expect("spawn wlel");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }

    /// build + run, returning (stdout, exit code of the binary)
    fn build_and_run(&self, name: &str) -> (String, i32) {
        let (out, err, code) = self.run_cli(&["build", &format!("{name}.wlel"), "-o", "bin"]);
        assert_eq!(code, Some(0), "build failed: {out}{err}");
        let run = Command::new(self.dir.join("bin")).output().expect("run");
        (
            String::from_utf8_lossy(&run.stdout).into_owned(),
            run.status.code().unwrap_or(-1),
        )
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// ---------------------------------------------------------------------------
// try operator: `return expr?` must re-wrap the payload

#[test]
fn return_try_result_rewraps_ok() {
    let c = Case::new(
        "ret_try_ok",
        r#"use std;
fn f() -> Result[int, string] { return Ok(5); }
fn g() -> Result[int, string] { return f()?; }
fn main() -> int {
    r := g();
    match r {
        Ok(v) => { std::println_int(v); }
        Err(e) => { std::println_str(e); }
    }
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("ret_try_ok");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.trim(), "5", "{out}");
}

#[test]
fn return_try_option_and_err_propagation() {
    let c = Case::new(
        "ret_try_opt",
        r#"use std;
fn some() -> Option[int] { return Some(7); }
fn none() -> Option[int] { return None; }
fn a() -> Option[int] { return some()?; }
fn b() -> Option[int] { return none()?; }
fn main() -> int {
    match a() { Some(v) => { std::println_int(v); } None => { std::println_str("none"); } }
    match b() { Some(v) => { std::println_int(v); } None => { std::println_str("none"); } }
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("ret_try_opt");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.replace('\n', "|"), "7|none|", "{out}");
}

#[test]
fn enum_constructor_as_call_argument_infers_from_parameter() {
    let c = Case::new(
        "arg_infer",
        r#"use std;
fn take(r: Result[int, string]) -> int { return r.unwrap(); }
fn opt(o: Option[int]) -> int { return o.unwrap(); }
fn main() -> int {
    std::println_int(take(Ok(5)));
    std::println_int(opt(Some(7)));
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("arg_infer");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.replace('\n', "|"), "5|7|", "{out}");
}

#[test]
fn compound_assignment_with_try() {
    let c = Case::new(
        "compound_try",
        r#"use std;
fn f() -> Result[int, string] { return Ok(3); }
fn g() -> Result[int, string] {
    x := 1;
    x += f()?;
    x *= f()?;
    return Ok(x);
}
fn main() -> int { std::println_int(g().unwrap()); return 0; }"#,
    );
    let (out, code) = c.build_and_run("compound_try");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.trim(), "12", "{out}");
}

#[test]
fn try_on_payloadless_variant_is_a_clean_error() {
    // used to panic with "index out of bounds" in the checker
    for src in [
        "enum Result[T, E] { Ok, Err(E) }
         fn g() -> Result[int, string] { return Ok; }
         fn h() -> Result[int, string] { x := g()?; return Ok(1); }
         fn main() -> int { return 0; }",
        "enum Result[T, E] { Ok(T), Err }
         fn g() -> Result[int, string] { return Ok(1); }
         fn h() -> Result[int, string] { x := g()?; return Ok(x); }
         fn main() -> int { return 0; }",
        "enum Option[T] { Some, None }
         fn g() -> Option[int] { return Some; }
         fn h() -> Option[int] { x := g()?; return Some(1); }
         fn main() -> int { return 0; }",
    ] {
        let msg = check_err(src);
        assert!(msg.contains("exactly one payload"), "{msg}");
    }
}

// ---------------------------------------------------------------------------
// parser hardening

#[test]
fn long_else_if_chain_is_rejected_not_crashing() {
    let mut src = String::from("fn main() -> int {\n    x := 1;\n    if x == 0 { return 0; }\n");
    for i in 1..8000 {
        src.push_str(&format!("    else if x == {i} {{ return {i}; }}\n"));
    }
    src.push_str("    return -1;\n}\n");
    let toks = Lexer::new(&src).tokenize().expect("lex");
    let (_, errs) = Parser::new(&toks).program();
    assert!(!errs.is_empty(), "deep chain must be rejected");
    assert!(errs.iter().any(|e| e.msg.contains("nesting too deep")), "{errs:?}");
}

#[test]
fn trailing_dot_is_a_syntax_error() {
    for src in [
        "fn main() -> int {\n    x := 1;\n    x.;\n    return 0;\n}\n",
        "fn main() -> int {\n    x := 1;\n    x. = 5;\n    return 0;\n}\n",
        "fn main() -> int {\n    x := 1;\n    x.5;\n    return 0;\n}\n",
    ] {
        let toks = Lexer::new(src).tokenize().expect("lex");
        let (_, errs) = Parser::new(&toks).program();
        assert!(!errs.is_empty(), "trailing dot must be rejected: {src}");
    }
}

#[test]
fn float_literal_overflow_is_rejected() {
    let toks = Lexer::new("fn main() -> int { x := 1e999; return 0; }")
        .tokenize()
        .expect_err("overflowing float literal must not lex");
    assert!(toks.msg.contains("out of range"), "{toks}");
}

// ---------------------------------------------------------------------------
// checker validation that used to reach cc

#[test]
fn c_keywords_and_runtime_symbols_are_rejected() {
    for src in [
        "fn main() -> int { int := 5; return int; }",
        "struct P { int: int }\nfn main() -> int { return 0; }",
        "fn f(long: int) -> int { return long; }\nfn main() -> int { return f(1); }",
        "fn printf(x: int) -> int { return x; }\nfn main() -> int { return 0; }",
        "fn wlel_alloc(n: int) -> *void { return 0 as *void; }\nfn main() -> int { return 0; }",
        "fn _wlel_sort() -> int { return 1; }\nfn main() -> int { return _wlel_sort(); }",
        "fn _wlel_thread_spawn() -> int { return 1; }\nfn main() -> int { return _wlel_thread_spawn(); }",
        "struct int { x: int }\nfn main() -> int { return 0; }",
        "enum E { int(int) }\nfn main() -> int { return 0; }",
    ] {
        let msg = check_err(src);
        assert!(msg.contains("collides") || msg.contains("reserved"), "{msg}");
    }
}

#[test]
fn duplicate_fields_and_type_collisions_are_rejected() {
    let msg = check_err("struct P { x: int, x: int }\nfn main() -> int { return 0; }");
    assert!(msg.contains("duplicate field 'x'"), "{msg}");
    let msg = check_err("struct X[T] { v: T }\nenum X[T] { A(T) }\nfn main() -> int { return 0; }");
    assert!(msg.contains("already declared as a struct"), "{msg}");
}

#[test]
fn main_signature_is_validated() {
    let msg = check_err("fn main(a: int) -> int { return a; }");
    assert!(msg.contains("main takes no parameters"), "{msg}");
    let msg = check_err("fn main() -> string { return \"x\"; }");
    assert!(msg.contains("main must return int"), "{msg}");
}

#[test]
fn assignment_target_must_be_an_lvalue() {
    for src in [
        "fn main() -> int { 1 = 2; return 0; }",
        "struct P { x: int }\nfn mk() -> P { return P { x: 1 }; }\nfn main() -> int { mk().x = 5; return 0; }",
        "fn main() -> int { x := 1; (x + 1) += 2; return 0; }",
    ] {
        let msg = check_err(src);
        assert!(msg.contains("assignment target"), "{msg}");
    }
}

#[test]
fn for_in_rejects_arrays_of_arrays_and_lenless_vecs() {
    let msg = check_err(
        "fn main() -> int { m := [[1, 2], [3, 4]]; n := 0; for row in m { n += 1; } return n; }",
    );
    assert!(msg.contains("arrays of arrays"), "{msg}");
    let msg = check_err(
        "struct S { data: *int }
         fn main() -> int { p := new(int, 2); s := S { data: p }; for x in s { } return 0; }",
    );
    assert!(msg.contains("int `len`"), "{msg}");
}

#[test]
fn array_values_are_not_first_class() {
    let msg = check_err(
        "fn main() -> int { a := [1, 2, 3]; let b: [int; 3] = a; return b[0]; }",
    );
    assert!(msg.contains("not first-class"), "{msg}");
    let msg = check_err("fn main() -> int { a := [1, 2, 3]; b := a; return b[0]; }");
    assert!(msg.contains("not first-class"), "{msg}");
    let msg = check_err("fn main() -> int { a := [1, 2, 3]; p := &a; return 0; }");
    assert!(msg.contains("address of an array"), "{msg}");
}

// ---------------------------------------------------------------------------
// codegen: arrays in value position, sort single evaluation

#[test]
fn array_literals_work_in_value_position() {
    let c = Case::new(
        "arr_value",
        r#"use std;
fn sum(p: *int, n: int) -> int {
    s := 0;
    i := 0;
    while i < n { s += p[i]; i += 1; }
    return s;
}
fn main() -> int {
    std::println_int([10, 20, 30][1]);
    std::println_int(sum([1, 2, 3], 3));
    std::println_int(std::len([1, 2, 3]));
    n := 0;
    for x in [1, 2, 3] { n += x; }
    std::println_int(n);
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("arr_value");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.replace('\n', "|"), "20|6|3|6|", "{out}");
}

#[test]
fn struct_field_array_before_element_type_builds() {
    // ty_embeds ignored array element types, so this emitted `Pt items[2]`
    // before Pt was defined
    let c = Case::new(
        "order",
        r#"struct Holder { items: [Pt; 2] }
struct Pt { x: int }
fn main() -> int {
    h := Holder { items: [Pt { x: 1 }, Pt { x: 2 }] };
    return h.items[1].x;
}"#,
    );
    let (_, code) = c.build_and_run("order");
    assert_eq!(code, 2);
}

#[test]
fn nested_arrays_build_and_index() {
    let c = Case::new(
        "nested",
        r#"fn main() -> int {
    let m: [[int; 2]; 2] = [[1, 2], [3, 4]];
    return m[0][1] + m[1][0];
}"#,
    );
    let (_, code) = c.build_and_run("nested");
    assert_eq!(code, 5);
}

#[test]
fn sort_evaluates_the_collection_once() {
    let c = Case::new(
        "sort_once",
        r#"use std;
fn cmp(a: int, b: int) -> int { return a - b; }
fn mk() -> Vec[int] {
    std::println_str("mk");
    v := vec_new[int]();
    v.push(3); v.push(1); v.push(2);
    return v;
}
fn main() -> int {
    std::sort(mk(), cmp);
    std::println_int(std::binary_search(mk(), 2, cmp));
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("sort_once");
    assert_eq!(code, 0, "{out}");
    // one "mk" per call: the collection is evaluated exactly once, and the
    // second call searches its own (sorted) vector
    // exactly two calls total (one per sort/search), never one per field
    assert_eq!(out.matches("mk").count(), 2, "{out}");
    assert!(out.lines().last().map(|l| l.parse::<i64>().is_ok()).unwrap_or(false), "{out}");
}

#[test]
fn sort_of_array_literal_builds() {
    let c = Case::new(
        "sort_lit",
        r#"use std;
fn cmp(a: int, b: int) -> int { return a - b; }
fn main() -> int {
    std::sort([3, 1, 2], cmp);
    std::println_int(std::binary_search([1, 2, 3], 2, cmp));
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("sort_lit");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.trim(), "1", "{out}");
}

#[test]
fn nul_escape_does_not_swallow_the_next_digit() {
    let c = Case::new(
        "nul_octal",
        r#"use std;
fn main() -> int {
    s := "a\01";
    std::println_int(std::strlen(s));
    std::println_int(s[1] as int);
    std::println_int(s[2] as int);
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("nul_octal");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.replace('\n', "|"), "1|0|49|", "{out}");
}

#[test]
fn print_bool_is_implemented() {
    let c = Case::new(
        "print_bool",
        "use std;\nfn main() -> int { std::print_bool(true); std::println_bool(false); return 0; }",
    );
    let (out, code) = c.build_and_run("print_bool");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.trim(), "truefalse");
}

// ---------------------------------------------------------------------------
// safe mode: negative sizes, compound division by zero

#[test]
fn safe_mode_reports_negative_allocation_sizes() {
    let c = gen_src(
        "fn main() -> int { p := new(int, -1); p[0] = 1; return 0; }",
        true,
    );
    assert!(c.contains("_wlel_nonneg"), "{c}");
    let c = gen_src("fn main() -> int { arena(-5) { x := 1; } return 0; }", true);
    assert!(c.contains("_wlel_nonneg"), "{c}");
}

#[test]
fn safe_mode_checks_compound_division_by_zero() {
    let c = gen_src(
        "fn main() -> int { x := 10; y := 0; x /= y; return x; }",
        true,
    );
    assert!(c.contains("_wlel_divz"), "{c}");
    let c = gen_src(
        "fn main() -> int { x := 10; y := 0; x %= y; return x; }",
        true,
    );
    assert!(c.contains("_wlel_divz"), "{c}");
}

// ---------------------------------------------------------------------------
// stdlib hardening

#[test]
fn std_string_helpers_survive_huge_arguments() {
    let c = Case::new(
        "std_huge",
        r#"use std;
fn main() -> int {
    std::println_int(str_find_from("abc", "c", 9223372036854775807));
    s := str_sub("hello", 2, 9223372036854775807);
    std::println_str(s);
    std::println_int(std::strlen(str_sub("hello", 2, -1)));
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("std_huge");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.replace('\n', "|"), "-1|llo|0|", "{out}");
}

// ---------------------------------------------------------------------------
// enum construction: a nested call in a payload must not erase the
// expected-type hint from the annotated let / return type

#[test]
fn variant_payload_nested_call_keeps_expected_type() {
    // `Err(g("x"))` has no payload type that mentions T, so the Ok slot can
    // only come from the function's declared return type. Checking the
    // nested call used to clear that hint (flagship/json.wlel regressed on it).
    let c = Case::new(
        "variant_nested_hint",
        r#"use std;
fn shout(s: string) -> string { return s + "!"; }
fn fail(msg: string) -> Result[int, string] {
    return Err(shout(msg));
}
fn main() -> int {
    match fail("boom") {
        Ok(v) => { std::println_int(v); }
        Err(e) => { std::println_str(e); }
    }
    return 0;
}"#,
    );
    let (out, code) = c.build_and_run("variant_nested_hint");
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.trim(), "boom!", "{out}");
}

#[test]
fn let_annotation_hint_survives_nested_variant_payload() {
    let src = r#"use std;
fn ident(s: string) -> string { return s; }
fn main() -> int {
    let r: Result[int, string] = Err(ident("bad"));
    match r {
        Ok(v) => { return v; }
        Err(e) => { std::println_str(e); }
    }
    return 0;
}"#;
    let mut p = parse(src);
    wlel::stdsrc::splice_std(&mut p);
    Checker::check(&mut p).expect("the let annotation must seed the Ok slot");
}

// ---------------------------------------------------------------------------
// LSP: JSON escapes and didChange diagnostics

#[test]
fn lsp_json_unicode_escape_keeps_following_characters() {
    use wlel::lsp::Json;
    let v = Json::parse("{\"a\":\"\\u0041B\"}").expect("parse");
    assert_eq!(v.get("a").and_then(|s| s.as_str()), Some("AB"));
    let v = Json::parse("{\"a\":\"\\u0041\"}").expect("parse");
    assert_eq!(v.get("a").and_then(|s| s.as_str()), Some("A"));
}

#[test]
fn lsp_did_change_publishes_new_diagnostics() {
    use wlel::lsp::{Json, Server};
    let mut srv = Server::new();
    let open = Json::parse(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///z.wlel","version":1,"text":"fn main() -> int { return 0; }"}}}"#,
    )
    .expect("open");
    srv.handle(&open);
    let change = Json::parse(
        r#"{"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///z.wlel","version":2},"contentChanges":[{"text":"fn main() -> int { return zzz; }"}]}}"#,
    )
    .expect("change");
    let msgs = srv.handle(&change);
    let diags = msgs
        .iter()
        .find(|m| m.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics"))
        .and_then(|m| m.ptr(&["params", "diagnostics"]))
        .and_then(|d| d.as_arr())
        .expect("diagnostics");
    assert_eq!(diags.len(), 1, "undefined name must be reported after didChange");
}
