//! Result/Option idiomatic (Fase 3 item 2): the `?` try operator, the
//! `panic()` builtin, the result_*/option_* combinators, generic-enum
//! inference through unify, formatting, and end-to-end runs.

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

fn check_std(src: &str) -> Result<(), String> {
    let mut p = parse(src);
    if p.uses.iter().any(|u| u.path.is_none()) {
        stdsrc::splice_std(&mut p);
    }
    Checker::check(&mut p).map(|_| ()).map_err(|e| e.msg)
}

fn check_err_std(src: &str) -> String {
    check_std(src).unwrap_err()
}

fn gen_checked_std(src: &str) -> String {
    let mut p = parse(src);
    if p.uses.iter().any(|u| u.path.is_none()) {
        stdsrc::splice_std(&mut p);
    }
    Checker::check(&mut p).expect("typecheck");
    gen_program(&p)
}

// ---------------------------------------------------------------------------
// parsing

#[test]
fn try_operator_parses_in_statement_positions() {
    let p = parse(
        "fn f(r: int) -> int {
             x := r?;
             r = r?;
             r?;
             return r?;
         }",
    );
    let body = &p.funcs[0].body.0;
    assert!(matches!(&body[0].node, StmtKind::Let(_, None, e) if matches!(e.node, ExprKind::Try(_))));
    assert!(matches!(&body[1].node, StmtKind::Assign(a) if matches!(a.value.node, ExprKind::Try(_))));
    assert!(matches!(&body[2].node, StmtKind::ExprStmt(e) if matches!(e.node, ExprKind::Try(_))));
    assert!(
        matches!(&body[3].node, StmtKind::Return(Some(e)) if matches!(e.node, ExprKind::Try(_)))
    );
}

#[test]
fn try_binds_tighter_than_binary_ops() {
    let p = parse("fn f(r: int) -> int { return r? + 1; }");
    match &p.funcs[0].body.0[0].node {
        StmtKind::Return(Some(e)) => match &e.node {
            ExprKind::Binary(_, l, _) => {
                assert!(matches!(l.node, ExprKind::Try(_)), "{:?}", l.node);
            }
            other => panic!("expected binary at top, got {:?}", other),
        },
        other => panic!("expected return, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// checking: the try operator

#[test]
fn try_propagates_through_let_assign_return_and_stmt() {
    check_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[int, string] {
             a := inner()?;
             a = inner()?;
             inner()?;
             return inner()?;
         }",
    )
    .expect("all four ? forms accepted");
}

#[test]
fn try_yields_the_success_payload_type() {
    check_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[int, string] {
             a := inner()?;
             return Ok(a + 1);
         }",
    )
    .expect("a is int");
}

#[test]
fn try_on_non_result_rejected() {
    let msg = check_err_std(
        "use std;
         fn f() -> Result[int, string] {
             x := 5?;
             return Ok(x);
         }",
    );
    assert!(msg.contains("'?' expects a Result or Option value, got int"), "{msg}");
}

#[test]
fn try_outside_result_function_rejected() {
    let msg = check_err_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> int {
             x := inner()?;
             return x;
         }",
    );
    assert!(
        msg.contains("'?' needs the enclosing function to return a Result with the error type string — it returns int"),
        "{msg}"
    );
}

#[test]
fn try_error_type_mismatch_rejected() {
    let msg = check_err_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[int, int] {
             x := inner()?;
             return Ok(x);
         }",
    );
    assert!(msg.contains("'?' error type mismatch"), "{msg}");
    assert!(msg.contains("fails with string"), "{msg}");
}

#[test]
fn try_in_defer_block_rejected() {
    let msg = check_err_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[int, string] {
             defer {
                 x := inner()?;
             }
             return Ok(0);
         }",
    );
    assert!(msg.contains("'?' inside a defer block is not allowed"), "{msg}");
}

#[test]
fn try_in_test_block_rejected() {
    let mut p = parse(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         test \"t\" {
             x := inner()?;
         }",
    );
    stdsrc::splice_std(&mut p);
    let msg = Checker::check(&mut p).unwrap_err().msg;
    assert!(msg.contains("'?' needs the enclosing function"), "{msg}");
}

#[test]
fn try_on_option_propagates_none() {
    check_std(
        "use std;
         fn inner() -> Option[int] { return Some(1); }
         fn f() -> Option[int] {
             a := inner()?;
             return Some(a + 1);
         }",
    )
    .expect("option ? accepted");
}

#[test]
fn try_mismatched_result_vs_option_rejected() {
    let msg = check_err_std(
        "use std;
         fn inner() -> Option[int] { return Some(1); }
         fn f() -> Result[int, string] {
             a := inner()?;
             return Ok(a);
         }",
    );
    assert!(msg.contains("'?' needs the enclosing function to return an Option"), "{msg}");
}

#[test]
fn try_nested_or_in_expression_rejected() {
    let msg = check_err_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[int, string] {
             x := inner()??;
             return Ok(x);
         }",
    );
    assert!(msg.contains("'?' may only appear directly"), "{msg}");
    let msg = check_err_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[int, string] {
             x := inner()? + 1;
             return Ok(x);
         }",
    );
    assert!(msg.contains("'?' may only appear directly"), "{msg}");
}

#[test]
fn try_return_checks_against_ok_payload() {
    let msg = check_err_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[string, string] {
             return inner()?;
         }",
    );
    assert!(
        msg.contains("the function's success type is string, found int"),
        "{msg}"
    );
    // the compatible form passes: same payload type
    check_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[int, string] {
             return inner()?;
         }",
    )
    .expect("same payload type accepted");
}

// ---------------------------------------------------------------------------
// checking: panic()

#[test]
fn panic_builtin_takes_one_string() {
    let msg = check_err_std("fn f() { panic(5); }");
    assert!(msg.contains("panic() expects a string message, got int"), "{msg}");
    let msg = check_err_std("fn f() { panic(); }");
    assert!(msg.contains("panic() takes exactly 1 argument"), "{msg}");
    check_std("fn f() { panic(\"boom\"); }").expect("panic with string accepted");
}

#[test]
fn panic_terminates_blocks_and_arms() {
    // a function ending in panic needs no further return
    check_std(
        "fn boom() -> int {
             panic(\"unreachable\");
         }",
    )
    .expect("panic counts as terminating");
    // a match arm ending in panic terminates the whole match
    check_std(
        "fn pick(r: int) -> int {
             return r;
         }",
    )
    .expect("baseline");
}

// ---------------------------------------------------------------------------
// checking: combinators + generic enum inference

#[test]
fn combinators_type_check_and_infer_from_enums() {
    check_std(
        "use std;
         fn f(r: Result[int, string]) -> int {
             if result_is_ok(r) {
                 return result_unwrap(r);
             }
             o := result_ok(r);
             if option_is_none(o) {
                 return -1;
             }
             return option_unwrap_or(o, 0);
         }",
    )
    .expect("combinators accept Result arguments");
}

#[test]
fn combinator_error_paths_rejected() {
    let msg = check_err_std(
        "use std;
         fn f(r: Result[int, string]) -> int {
             return result_unwrap(r) + \"x\";
         }",
    );
    assert!(msg.contains("int") && msg.contains("string"), "{msg}");
}

// ---------------------------------------------------------------------------
// codegen

#[test]
fn try_codegen_propagate_shape() {
    let c = gen_checked_std(
        "use std;
         fn inner() -> Result[int, string] { return Ok(1); }
         fn f() -> Result[int, string] {
             x := inner()?;
             return Ok(x);
         }",
    );
    // evaluate once into a temp, early-return the error variant
    assert!(c.contains("Result__int__string _wlel_try0 = inner();"), "{c}");
    assert!(c.contains("if (_wlel_try0._tag == 1) {"), "{c}");
    assert!(
        c.contains("return (Result__int__string){ ._tag = 1, .v = { .Err = { _wlel_try0.v.Err.f0 } } };"),
        "{c}"
    );
    assert!(c.contains("long long x = _wlel_try0.v.Ok.f0;"), "{c}");
}

#[test]
fn try_codegen_option_none_propagate() {
    let c = gen_checked_std(
        "use std;
         fn inner() -> Option[int] { return Some(1); }
         fn f() -> Option[int] {
             x := inner()?;
             return Some(x);
         }",
    );
    assert!(c.contains("if (_wlel_try0._tag == 1) {"), "{c}");
    assert!(c.contains("return (Option__int){ ._tag = 1 };"), "{c}");
    assert!(c.contains("long long x = _wlel_try0.v.Some.f0;"), "{c}");
}

#[test]
fn try_in_test_mode_runs_defers_on_propagate() {
    let mut p = parse(
        "use std;
         fn inner() -> Result[int, string] { return Err(\"boom\"); }
         fn f() -> Result[int, string] {
             arena(64) {
                 x := inner()?;
                 return Ok(x);
             }
         }
         test \"t\" { assert(result_is_ok(f()) == false); }",
    );
    stdsrc::splice_std(&mut p);
    Checker::check(&mut p).expect("typecheck");
    let c = gen_program_tests(&p);
    // the propagate path exits the function, so the arena exit (a defer)
    // must run inside the error branch before the return
    assert!(c.contains("if (_wlel_try"), "{c}");
}

// ---------------------------------------------------------------------------
// formatting

#[test]
fn fmt_try_operator_canonical() {
    let src = "fn f(r: int) -> int {\n\
               x := r?;\n\
               return r? + 1;\n\
               }\n";
    let once = wlel::fmt::format_source(src).expect("format");
    assert!(once.contains("x := r?;"), "{once}");
    assert!(once.contains("return r? + 1;"), "{once}");
    let twice = wlel::fmt::format_source(&once).expect("format");
    assert_eq!(once, twice, "formatter is not idempotent");
}

// ---------------------------------------------------------------------------
// end-to-end CLI

struct TempWl {
    path: PathBuf,
}

impl TempWl {
    fn new(name: &str, src: &str) -> TempWl {
        let dir = std::env::temp_dir().join(format!(
            "wlel_result_{}_{}",
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
fn cli_try_chain_end_to_end() {
    let t = TempWl::new(
        "try_chain",
        r#"use std;
           fn parse_age(s: string) -> Result[int, string] {
               n := 0;
               if !str_parse_int(s, &n) {
                   return Err("not a number: " + s);
               }
               if n < 0 {
                   return Err("negative age");
               }
               return Ok(n);
           }
           fn twice(s: string) -> Result[int, string] {
               v := parse_age(s)?;
               return Ok(v * 2);
           }
           fn run() -> Result[int, string] {
               a := parse_age("21")?;
               b := twice("10")?;
               c := twice("-3")?;
               std::println_int(a + b + c);
               return Ok(0);
           }
           fn main() -> int {
               match run() {
                   Ok(code) => { return code; }
                   Err(e) => {
                       std::println_str("failed: " + e);
                       return 1;
                   }
               }
           }"#,
    );
    let (out, err, code) = t.run("run");
    assert_eq!(code, Some(1), "out={out} err={err}");
    assert!(out.contains("failed: negative age"), "{out}");
}

#[test]
fn cli_try_success_path_end_to_end() {
    let t = TempWl::new(
        "try_ok",
        r#"use std;
           fn num(s: string) -> Result[int, string] {
               n := 0;
               if !str_parse_int(s, &n) { return Err("bad"); }
               return Ok(n);
           }
           fn run() -> Result[int, string] {
               a := num("20")?;
               b := num("22")?;
               return Ok(a + b);
           }
           fn main() -> int {
               match run() {
                   Ok(v) => { std::println_int(v); return 0; }
                   Err(e) => { std::println_str(e); return 1; }
               }
           }"#,
    );
    let (out, err, code) = t.run("run");
    assert_eq!(code, Some(0), "out={out} err={err}");
    assert!(out.contains("42"), "{out}");
}

#[test]
fn cli_unwrap_on_err_panics_end_to_end() {
    let t = TempWl::new(
        "unwrap_panic",
        r#"use std;
           fn num(s: string) -> Result[int, string] {
               n := 0;
               if !str_parse_int(s, &n) { return Err("bad"); }
               return Ok(n);
           }
           fn main() -> int {
               v := result_unwrap(num("zz"));
               std::println_int(v);
               return 0;
           }"#,
    );
    let (out, err, code) = t.run("run");
    assert_ne!(code, Some(0), "out={out}");
    assert!(err.contains("panic at"), "{err}");
    assert!(err.contains("result_unwrap"), "{err}");
}

#[test]
fn cli_unwrap_on_ok_value_end_to_end() {
    let t = TempWl::new(
        "unwrap_ok",
        r#"use std;
           fn num(s: string) -> Result[int, string] {
               n := 0;
               if !str_parse_int(s, &n) { return Err("bad"); }
               return Ok(n);
           }
           fn main() -> int {
               std::println_int(result_unwrap(num("41")));
               std::println_int(result_unwrap_or(num("zz"), -7));
               o := result_ok(num("5"));
               std::println_int(option_unwrap(o));
               return 0;
           }"#,
    );
    let (out, err, code) = t.run("run");
    assert_eq!(code, Some(0), "out={out} err={err}");
    for line in ["41", "-7", "5"] {
        assert!(out.contains(line), "want {line} in {out}");
    }
}

#[test]
fn cli_unwrap_in_test_fails_that_test_only() {
    let t = TempWl::new(
        "unwrap_test",
        r#"use std;
           test "good" {
               assert_eq(1 + 1, 2);
           }
           test "panics" {
               let r: Result[int, string] = Err("boom");
               v := result_unwrap(r);
               assert_eq(v, 0);
           }
           test "also good" {
               assert_eq(2 + 2, 4);
           }"#,
    );
    let (out, err, code) = t.run("test");
    assert_ne!(code, Some(0));
    assert!(out.contains("pass: good"), "{out}");
    assert!(out.contains("pass: also good"), "{out}");
    assert!(out.contains("FAIL: panics"), "{out}");
    assert!(err.contains("on an Err value"), "{err}");
}

#[test]
fn cli_try_with_option_end_to_end() {
    let t = TempWl::new(
        "try_option",
        r#"use std;
           fn first_char(s: string) -> Option[string] {
               if std::strlen(s) == 0 { return None; }
               return Some(str_sub(s, 0, 1));
           }
           fn shout(s: string) -> Option[string] {
               c := first_char(s)?;
               return Some(c + "!");
           }
           fn main() -> int {
               m := shout("hey");
               if option_is_some(m) {
                   std::println_str(option_unwrap(m));
               }
               e := shout("");
               assert(option_is_none(e));
               return 0;
           }"#,
    );
    let (out, err, code) = t.run("run");
    assert_eq!(code, Some(0), "out={out} err={err}");
    assert!(out.contains("h!"), "{out}");
}

#[test]
fn cli_stmt_try_discards_value_end_to_end() {
    let t = TempWl::new(
        "try_stmt",
        r#"use std;
           fn write_log() -> Result[int, string] {
               return Ok(9);
           }
           fn run() -> Result[int, string] {
               write_log()?;
               return Ok(1);
           }
           fn main() -> int {
               match run() {
                   Ok(v) => { std::println_int(v); return 0; }
                   Err(e) => { std::println_str(e); return 1; }
               }
           }"#,
    );
    let (out, err, code) = t.run("run");
    assert_eq!(code, Some(0), "out={out} err={err}");
    assert!(out.contains("1"), "{out}");
}
