//! Tagged enums + match (Fase 3 item 1): parsing, checking, exhaustiveness,
//! codegen shape, formatting, and end-to-end runs incl. the std Result dogfood.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use wlel::ast::*;
use wlel::checker::Checker;
use wlel::codegen::{gen_program, gen_program_tests};
use wlel::lexer::Lexer;
use wlel::parser::Parser;
use wlel::stdsrc;

fn parse(src: &str) -> Program {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
    p
}

#[allow(dead_code)]
fn gen(src: &str) -> String {
    gen_program(&parse(src))
}

fn check(src: &str) -> Result<(), String> {
    let mut p = parse(src);
    Checker::check(&mut p).map(|_| ()).map_err(|e| e.msg)
}

/// parse + check with the embedded std spliced in (same as the CLI front end),
/// so programs using Result/Option type-check
fn check_std(src: &str) -> Result<(), String> {
    let mut p = parse(src);
    if p.uses.iter().any(|u| u.path.is_none()) {
        let std_prog = stdsrc::parse_std();
        p.structs.splice(0..0, std_prog.structs);
        p.enums.splice(0..0, std_prog.enums);
        p.funcs.splice(0..0, std_prog.funcs);
    }
    Checker::check(&mut p).map(|_| ()).map_err(|e| e.msg)
}

fn check_err(src: &str) -> String {
    check(src).unwrap_err()
}

fn gen_checked(src: &str) -> String {
    let mut p = parse(src);
    Checker::check(&mut p).expect("typecheck");
    gen_program(&p)
}

// ---------------------------------------------------------------------------
// parsing
// ---------------------------------------------------------------------------

#[test]
fn enum_decl_parses_variants_and_payloads() {
    let p = parse(
        "enum Shape {
             Circle(float),
             Rect(float, float),
             Point
         }",
    );
    assert_eq!(p.enums.len(), 1);
    let e = &p.enums[0];
    assert_eq!(e.name, "Shape");
    assert_eq!(e.variants.len(), 3);
    assert_eq!(e.variants[0].name, "Circle");
    assert_eq!(e.variants[0].payloads, vec!["float"]);
    assert_eq!(e.variants[1].name, "Rect");
    assert_eq!(e.variants[1].payloads, vec!["float", "float"]);
    assert_eq!(e.variants[2].name, "Point");
    assert!(e.variants[2].payloads.is_empty());
}

#[test]
fn match_stmt_parses_patterns_and_bodies() {
    let p = parse(
        "enum E { A(int), B }
         fn f(e: E) -> int {
             match e {
                 A(x) => { return x; }
                 B => { return 0; }
             }
         }",
    );
    let body = &p.funcs[0].body.0;
    match &body[0].node {
        StmtKind::Match(m) => {
            assert!(matches!(m.scrutinee.node, ExprKind::Ident(ref n) if n == "e"));
            assert_eq!(m.arms.len(), 2);
            assert!(
                matches!(&m.arms[0].pattern, Pattern::Variant { name, binds, .. }
                    if name == "A" && binds.len() == 1 && binds[0].name == "x")
            );
            assert!(matches!(&m.arms[1].pattern, Pattern::Variant { name, .. } if name == "B"));
            assert!(matches!(&m.arms[0].body, MatchBody::Block(_)));
        }
        other => panic!("expected match stmt, got {:?}", other),
    }
}

#[test]
fn wildcard_arm_and_expr_bodies_parse() {
    let p = parse(
        "enum E { A, B }
         fn f(e: E) -> int {
             match e {
                 A => 1,
                 _ => 0,
             }
         }",
    );
    match &p.funcs[0].body.0[0].node {
        StmtKind::Match(m) => {
            assert!(matches!(&m.arms[1].pattern, Pattern::Wildcard));
            assert!(matches!(&m.arms[0].body, MatchBody::Expr(_)));
        }
        other => panic!("expected match stmt, got {:?}", other),
    }
}

#[test]
fn match_in_return_position_parses() {
    let p = parse(
        "enum E { A, B }
         fn f(e: E) -> int {
             return match e {
                 A => 1,
                 B => 2,
             };
         }",
    );
    match &p.funcs[0].body.0[0].node {
        StmtKind::Return(Some(e)) => matches!(e.node, ExprKind::Match(_))
            .then_some(())
            .expect("return match"),
        other => panic!("expected return match, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// checking
// ---------------------------------------------------------------------------

#[test]
fn match_requires_enum_scrutinee() {
    let e = check_err(
        "fn f(x: int) -> int {
             match x {
                 _ => 0,
             }
         }",
    );
    assert!(e.contains("match expects an enum value, got int"), "{e}");
}

#[test]
fn match_not_exhaustive_names_missing_variants() {
    let e = check_err(
        "enum E { A, B, C }
         fn f(e: E) -> int {
             match e {
                 A => 1,
                 B => 2,
             }
         }",
    );
    assert!(e.contains("not exhaustive: missing variant(s) C"), "{e}");
}

#[test]
fn duplicate_arm_rejected() {
    let e = check_err(
        "enum E { A, B }
         fn f(e: E) -> int {
             match e {
                 A => 1,
                 A => 2,
                 B => 3,
             }
         }",
    );
    assert!(e.contains("duplicate arm for variant 'A'"), "{e}");
}

#[test]
fn arm_after_wildcard_is_unreachable() {
    let e = check_err(
        "enum E { A, B }
         fn f(e: E) -> int {
             match e {
                 _ => 0,
                 A => 1,
             }
         }",
    );
    assert!(e.contains("arm after '_' wildcard is unreachable"), "{e}");
}

#[test]
fn wrong_payload_arity_rejected() {
    let e = check_err(
        "enum E { Pair(int, int), B }
         fn f(e: E) -> int {
             match e {
                 Pair(x) => x,
                 B => 0,
             }
         }",
    );
    assert!(e.contains("carries 2 payload value(s), pattern binds 1"), "{e}");
}

#[test]
fn unknown_variant_rejected() {
    let e = check_err(
        "enum E { A, B }
         fn f(e: E) -> int {
             match e {
                 A => 1,
                 Nope => 2,
             }
         }",
    );
    assert!(e.contains("has no variant 'Nope'"), "{e}");
}

#[test]
fn value_form_arms_share_one_type() {
    let e = check_err(
        "enum E { A, B }
         fn f(e: E) -> int {
             return match e {
                 A => 1,
                 B => \"x\",
             };
         }",
    );
    assert!(e.contains("match arm 2: expected int, got string"), "{e}");
}

#[test]
fn value_form_rejects_void_arm() {
    let e = check_err(
        "enum E { A, B }
         fn f(e: E) -> int {
             return match e {
                 A => wlel_print_int(1),
                 B => 2,
             };
         }",
    );
    assert!(e.contains("match arms cannot produce void"), "{e}");
}

#[test]
fn value_form_rejects_array_result() {
    let e = check_err(
        "enum E { A, B }
         fn f(e: E) -> int {
             let x: int = match e {
                 A => [1, 2, 3],
                 B => [4, 5, 6],
             };
             return 0;
         }",
    );
    assert!(e.contains("(an array) cannot be assigned"), "{e}");
}

#[test]
fn wildcard_arm_body_is_checked() {
    // regression: the wildcard arm used to skip the checker entirely, so
    // undefined names and bad builtin calls slipped through to C
    let e = check_err(
        "enum E { A, B }
         fn f(e: E) -> int {
             match e {
                 A => 1,
                 _ => nope,
             }
         }",
    );
    assert!(e.contains("undefined variable 'nope'"), "{e}");
}

#[test]
fn wildcard_arm_assert_eq_type_checked() {
    let e = check_err(
        "enum E { A, B }
         fn f(e: E) -> int {
             match e {
                 A => 1,
                 _ => assert_eq(1, \"x\"),
             }
         }",
    );
    assert!(e.contains("assert_eq() supports numbers, bools and strings"), "{e}");
}

#[test]
fn binding_scoped_to_its_arm() {
    let e = check_err(
        "enum E { A(int), B }
         fn f(e: E) -> int {
             match e {
                 A(x) => x,
                 B => 0,
             }
             return x;
         }",
    );
    assert!(e.contains("undefined variable 'x'"), "{e}");
}

#[test]
fn constructor_payload_type_checked() {
    let e = check_err(
        "enum E { A(int), B }
         fn f() -> E {
             return A(\"no\");
         }",
    );
    assert!(e.contains("payload 1 expects int"), "{e}");
}

#[test]
fn enum_field_access_rejected() {
    let e = check_err(
        "enum E { A }
         fn f(e: E) -> int {
             return e.x;
         }",
    );
    assert!(e.contains("destructure it with match"), "{e}");
}

#[test]
fn result_dogfood_std() {
    assert!(check_std(
        "use std;
         fn parse(raw: string) -> Result[int, string] {
             if raw == \"\" {
                 return Err(\"empty\");
             }
             return Ok(42);
         }
         fn describe(r: Result[int, string]) -> string {
             return match r {
                 Ok(v) => std::format(\"ok({})\", v),
                 Err(e) => std::format(\"err({})\", e),
             };
         }
         fn main() -> int {
             std::println_str(describe(parse(\"\")));
             std::println_str(describe(parse(\"4x\")));
             return 0;
         }"
    )
    .is_ok());
}

// ---------------------------------------------------------------------------
// codegen
// ---------------------------------------------------------------------------

#[test]
fn enum_codegen_emits_tagged_union() {
    let c = gen_checked(
        "enum Shape { Circle(float), Point }
         fn main() -> int {
             s := Circle(2.0);
             return 0;
         }",
    );
    assert!(c.contains("typedef struct Shape Shape;"), "{c}");
    assert!(c.contains("int _tag;"), "{c}");
    assert!(c.contains("union {"), "{c}");
    assert!(c.contains("} Circle;"), "{c}");
    assert!(
        c.contains("(Shape){ ._tag = 0, .v = { .Circle = { .f0 = 2.0 } } }"),
        "{c}"
    );
}

#[test]
fn match_codegen_if_else_chain_reads_payloads() {
    let c = gen_checked(
        "enum Shape { Circle(float), Point }
         fn area(s: Shape) -> float {
             return match s {
                 Circle(r) => r * r,
                 Point => 0.0,
             };
         }",
    );
    assert!(c.contains("double _wlel_mv0;"), "{c}");
    assert!(c.contains("if (_wlel_m1._tag == 0) {"), "{c}");
    assert!(c.contains("double r = _wlel_m1.v.Circle.f0;"), "{c}");
    assert!(c.contains("_wlel_mv0 = (r * r);"), "{c}");
    assert!(c.contains("} else if (_wlel_m1._tag == 1) {"), "{c}");
    assert!(c.contains("_wlel_mv0 = 0.0;"), "{c}");
    assert!(c.contains("return _wlel_mv0;"), "{c}");
}

#[test]
fn wildcard_arm_assert_eq_rewrites_in_codegen() {
    // regression: assert_eq inside a wildcard arm reached C unrewritten
    let mut p = parse(
        "enum E { A, B }
         fn main() -> int {
             match A {
                 A => assert_eq(1, 1),
                 _ => assert_eq(2, 2),
             }
             return 0;
         }",
    );
    p.funcs[0].file = "t.wl".into();
    Checker::check(&mut p).expect("typecheck");
    let c = gen_program(&p);
    assert_eq!(c.matches("_wlel_assert((").count(), 2, "{c}");
    assert!(!c.contains("assert_eq("), "{c}");
    assert!(c.contains("_wlel_assert((2 == 2), \"t.wl\","), "{c}");
}

#[test]
fn test_block_match_assert_eq_rewrites() {
    let mut p = parse(
        "enum E { A(int), B }
         fn main() -> int { return 0; }
         test \"wildcard\" {
             match A(7) {
                 A(v) => assert_eq(v, 7),
                 _ => assert_eq(0, 1),
             }
         }",
    );
    p.funcs[0].file = "t.wl".into();
    p.tests[0].file = "t.wl".into();
    Checker::check(&mut p).expect("typecheck");
    let c = gen_program_tests(&p);
    assert!(c.contains("_wlel_assert((v == 7), \"t.wl\","), "{c}");
    assert!(c.contains("_wlel_assert((0 == 1), \"t.wl\","), "{c}");
}

// ---------------------------------------------------------------------------
// formatting
// ---------------------------------------------------------------------------

#[test]
fn fmt_enum_and_match_canonical() {
    let src = "enum Shape { Circle(float), Rect(float, float), Point }\n\
               fn f(s: Shape) -> string {\n\
               match s { Circle(_) => { return \"c\"; } _ => { return \"?\"; } }\n\
               }\n\
               fn g(s: Shape) -> float {\n\
               return match s { Circle(r) => r, Rect(w, h) => w * h, Point => 0.0, };\n\
               }\n";
    let out = {
        let once = wlel::fmt::format_source(src).expect("format");
        let twice = wlel::fmt::format_source(&once).expect("format");
        assert_eq!(once, twice, "formatter is not idempotent");
        once
    };
    // enum: one variant per line, no trailing comma
    assert!(out.contains("enum Shape {\n    Circle(float),\n    Rect(float, float),\n    Point\n}"), "{out}");
    // statement form: block bodies expand, arm per line
    assert!(out.contains("match s {\n        Circle(_) => {\n            return \"c\";\n        }\n        _ => {\n            return \"?\";\n        }\n    }"), "{out}");
    // value form: expression arms stay on one line with trailing comma
    assert!(out.contains("return match s {\n        Circle(r) => r,\n        Rect(w, h) => w * h,\n        Point => 0.0,\n    };"), "{out}");
}

// ---------------------------------------------------------------------------
// end-to-end CLI
// ---------------------------------------------------------------------------

struct TempWl {
    path: PathBuf,
}

impl TempWl {
    fn new(name: &str, src: &str) -> TempWl {
        let dir = std::env::temp_dir().join(format!(
            "wlel_enum_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join(format!("{name}.wl"));
        fs::write(&path, src).expect("write temp .wl");
        TempWl { path }
    }

    fn run(&self, mode: &str) -> (String, String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args([mode, self.path.to_str().unwrap()])
            .output()
            .expect("spawn wlel");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }
}

impl Drop for TempWl {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.path.parent().unwrap());
    }
}

#[test]
fn cli_run_enum_program() {
    let t = TempWl::new(
        "run_enum",
        r#"use std;
           enum Op { Add, Push(int) }
           fn apply(op: Op) -> int {
               return match op {
                   Add => 10,
                   Push(v) => v,
               };
           }
           fn main() -> int {
               std::println_int(apply(Add));
               std::println_int(apply(Push(42)));
               return 0;
           }"#,
    );
    let (out, err, code) = t.run("run");
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("10"), "{out}");
    assert!(out.contains("42"), "{out}");
}

#[test]
fn cli_test_result_and_wildcard_assert_eq() {
    // dogfood: std Result + assert_eq inside wildcard arms must pass AND a
    // failing assert in a wildcard arm must fail the suite with file:line
    let t = TempWl::new(
        "test_enum",
        r#"use std;
           fn parse(raw: string) -> Result[int, string] {
               if raw == "" {
                   return Err("empty");
               }
               return Ok(42);
           }
           test "ok path" {
               match parse("42") {
                   Ok(v) => assert_eq(v, 42),
                   Err(e) => assert_eq(e, "unreachable"),
               }
           }
           test "wildcard arm" {
               match parse("") {
                   Ok(_) => assert_eq(0, 1),
                   _ => assert_eq(1, 1),
               }
           }"#,
    );
    let (out, err, code) = t.run("test");
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("pass: ok path"), "{out}");
    assert!(out.contains("pass: wildcard arm"), "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

#[test]
fn cli_test_failing_wildcard_assert_reports_file_line() {
    let t = TempWl::new(
        "fail_enum",
        r#"enum E { A, B }
           test "fails in wildcard" {
               match B {
                   A => assert_eq(1, 1),
                   _ => assert_eq(0, 1),
               }
           }"#,
    );
    let (out, _err, code) = t.run("test");
    assert_eq!(code, Some(1), "stdout: {out}");
    assert!(out.contains("FAIL: fails in wildcard"), "{out}");
}
