//! The 1.0-freeze decisions recorded in `docs/spec.md` §17, locked end to end:
//! mixed float widths (wider wins, literals adapt, f32→f64 widens) and
//! explicit array decay (`arr as *T` anywhere).

use std::fs;
use std::process::Command;

use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::parser::Parser;
use wlel::stdsrc::splice_std;

fn front(src: &str) -> Result<String, String> {
    let toks = Lexer::new(src).tokenize().map_err(|e| e.to_string())?;
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    splice_std(&mut p);
    for f in p.funcs.iter_mut() {
        if f.file.is_empty() {
            f.file = "freeze.wl".into();
        }
    }
    Checker::check(&mut p).map_err(|e| e.msg)?;
    Ok(gen_program(&p))
}

fn check_err_msg(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    splice_std(&mut p);
    for f in p.funcs.iter_mut() {
        if f.file.is_empty() {
            f.file = "freeze.wl".into();
        }
    }
    Checker::check(&mut p).expect_err("must be rejected").msg
}

/// run a source through `wlel run`, returning (ok, stdout, stderr)
fn run_cli(name: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_freeze_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join(format!("{name}.wl"));
    fs::write(&path, src).expect("write source");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    let out = (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
    );
    let _ = fs::remove_dir_all(&dir);
    out
}

// ---------------------------------------------------------------------------
// §4.3 / §17.2 — mixed float widths: the wider operand wins

#[test]
fn mixed_float_widths_take_the_wider_side() {
    // f32 + f64 is f64 regardless of operand order
    let c = front(
        "fn main() -> int {
             a := 1.5f32;
             b := 2.5;
             let x: float = a + b;
             let y: float = b + a;
             return (x + y) as int;
         }",
    )
    .expect("f64 wins in both orders");
    assert!(c.contains("double x ="), "{c}");
    assert!(c.contains("double y ="), "{c}");

    // f32 + f32 stays f32 (no silent widening through C promotion)
    let c = front(
        "fn main() -> int {
             a := 1.5f32;
             b := 2.5f32;
             let x: f32 = a + b;
             return x as int;
         }",
    )
    .expect("f32 + f32 is f32");
    assert!(c.contains("float x ="), "{c}");
}

#[test]
fn float_literal_adapts_to_the_other_operand() {
    // an unsuffixed literal must not promote f32 arithmetic to double
    let c = front(
        "fn main() -> int {
             a := 1.5f32;
             let x: f32 = a + 1.5;
             let y: f32 = a * 2.0 + 1.0;
             return (x + y) as int;
         }",
    )
    .expect("literals adapt to f32");
    assert!(c.contains("float x = (a + (float)(1.5));"), "{c}");
    assert!(c.contains("float y ="), "{c}");
    assert!(!c.contains("double y ="), "{c}");
}

#[test]
fn narrowing_to_f32_still_needs_a_cast() {
    let msg = check_err_msg(
        "fn main() -> int {
             a := 1.5f32;
             b := 2.5;
             let c: f32 = a + b;
             return 0;
         }",
    );
    assert!(msg.contains("cannot initialize 'c' of type f32 with float"), "{msg}");

    let msg = check_err_msg("fn main() -> int { let a: float = 1.0; let b: f32 = a; return 0; }");
    assert!(msg.contains("cannot initialize 'b' of type f32 with float"), "{msg}");
}

#[test]
fn f32_widens_into_f64_assignments_and_arguments() {
    // §4.1 rule 3: f32 -> f64 is implicit, like int -> float
    front(
        "use std;
         fn takes(x: float) -> float { return x; }
         fn main() -> int {
             a := 1.5f32;
             let w: float = a;
             std::println_float(takes(a));
             return w as int;
         }",
    )
    .expect("f32 widens to f64");
}

#[test]
fn f32_values_compare_against_f64_and_literals() {
    let (ok, out, err) = run_cli(
        "cmp",
        "use std;
         fn main() -> int {
             let small: f32 = 0.5;
             let big: float = 0.75;
             if small < big { std::println_str(\"lt\"); }
             if big > small { std::println_str(\"gt\"); }
             if small == 0.5 { std::println_str(\"eq-literal\"); }
             if small == -0.5 { std::println_str(\"bad\"); }
             assert_eq(small, 0.5f32);
             return 0;
         }",
    );
    assert!(ok, "run failed: {err}");
    assert_eq!(out, "lt\ngt\neq-literal\n", "stderr: {err}");
}

#[test]
fn float_literal_comparison_is_not_promoted_to_double() {
    // 0.1 is not exactly representable; comparing an f32 against a *double*
    // 0.1 would be false, so the literal must be cast to f32
    let c = front(
        "fn main() -> int {
             let a: f32 = 0.1;
             if a == 0.1 { return 1; }
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("(a == (float)(0.1))"), "{c}");
}

#[test]
fn f32_compound_assignment_computes_at_f32_width() {
    // `x += 1.5` is `x = x + 1.5` (§5.6), so the literal adapts to f32
    let c = front(
        "fn main() -> int {
             f := 1.5f32;
             f += 2.5;
             f *= 2;
             return f as int;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("f += (float)(2.5);"), "{c}");
}

#[test]
fn assert_eq_accepts_widened_float_pairs() {
    // assert_eq mirrors `==` exactly (§12.1)
    let (ok, out, err) = run_cli(
        "assert_eq",
        "use std;
         fn main() -> int {
             let small: f32 = 0.5;
             let big: float = 0.5;
             assert_eq(small, big);
             assert_eq(big, small);
             assert_eq(small, 0.5);
             std::println_str(\"ok\");
             return 0;
         }",
    );
    assert!(ok, "run failed: {err}");
    assert_eq!(out, "ok\n", "stderr: {err}");
}

// ---------------------------------------------------------------------------
// §4.4 / §17.4 — explicit array decay

#[test]
fn array_decays_explicitly_anywhere() {
    let c = front(
        "fn main() -> int {
             let arr: [int; 3] = [10, 20, 30];
             let p: *int = arr as *int;
             return p[2];
         }",
    )
    .expect("explicit decay is legal");
    assert!(c.contains("long long* p = (long long*)(arr);"), "{c}");

    let (ok, out, err) = run_cli(
        "decay",
        "use std;
         fn main() -> int {
             let arr: [int; 3] = [10, 20, 30];
             let p: *int = arr as *int;
             std::println_int(p[0]);
             std::println_int(p[2]);
             return 0;
         }",
    );
    assert!(ok, "run failed: {err}");
    assert_eq!(out, "10\n30\n", "stderr: {err}");
}

#[test]
fn array_decay_requires_matching_element_types() {
    let msg = check_err_msg(
        "fn main() -> int {
             let m: [[int; 2]; 2] = [[1, 2], [3, 4]];
             let p: *int = m as *int;
             return 0;
         }",
    );
    assert!(msg.contains("invalid cast from [[int; 2]; 2] to *int"), "{msg}");

    let msg = check_err_msg(
        "fn main() -> int {
             let a: [int; 2] = [1, 2];
             let p: *u8 = a as *u8;
             return 0;
         }",
    );
    assert!(msg.contains("invalid cast from [int; 2] to *u8"), "{msg}");
}

#[test]
fn array_to_non_pointer_cast_stays_rejected() {
    let msg = check_err_msg(
        "fn main() -> int {
             let a: [int; 2] = [1, 2];
             let n: int = a as int;
             return n;
         }",
    );
    assert!(msg.contains("invalid cast from [int; 2] to int"), "{msg}");
}
