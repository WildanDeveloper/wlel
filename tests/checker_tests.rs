use wlel::checker::Checker;
use wlel::lexer::Lexer;
use wlel::parser::Parser;

fn check(src: &str) -> Result<(), String> {
    let toks = Lexer::new(src).tokenize().expect("lex");
    let p = Parser::new(&toks).program().expect("parse");
    Checker::check(&p).map_err(|e| e.msg)
}

#[test]
fn valid_program_passes() {
    assert!(check(
        "fn add(a: int, b: int) -> int { return a + b; }
         fn main() -> int {
             x := add(1, 2);
             if x > 0 { return 0; }
             return 1;
         }"
    ).is_ok());
}

#[test]
fn undefined_variable() {
    let e = check("fn main() -> int { return nope; }").unwrap_err();
    assert!(e.contains("undefined variable 'nope'"), "{e}");
}

#[test]
fn type_mismatch_on_let_annotation() {
    let e = check(r#"fn main() -> int { let s: string = 5; return 0; }"#).unwrap_err();
    assert!(e.contains("cannot initialize"), "{e}");
}

#[test]
fn unknown_type() {
    let e = check("fn f(x: banana) -> int { return 0; }").unwrap_err();
    assert!(e.contains("unknown type 'banana'"), "{e}");
}

#[test]
fn call_unknown_function() {
    let e = check("fn main() -> int { ghost(); return 0; }").unwrap_err();
    assert!(e.contains("undefined function 'ghost'"), "{e}");
}

#[test]
fn arg_count_and_types() {
    let e = check(
        "fn add(a: int, b: int) -> int { return a + b; }
         fn main() -> int { return add(1); }",
    )
    .unwrap_err();
    assert!(e.contains("takes 2 argument(s)"), "{e}");

    let e2 = check(
        "fn take_str(s: string) -> int { return 0; }
         fn main() -> int { return take_str(3); }",
    )
    .unwrap_err();
    assert!(e2.contains("expected string"), "{e2}");
}

#[test]
fn mod_needs_ints() {
    let e = check("fn main() -> float { return 1.5 % 2.0; }").unwrap_err();
    assert!(e.contains("'Mod'"), "{e}");
}

#[test]
fn comparison_returns_bool() {
    assert!(check(
        "fn main() -> bool { return 1 < 2 && true; }"
    ).is_ok());
    let e = check("fn main() -> int { return (1 < 2) + 1; }").unwrap_err();
    assert!(e.contains("cannot apply"), "{e}");
}

#[test]
fn return_type_mismatch() {
    let e = check("fn f() -> int { return 1.5; }").unwrap_err();
    assert!(e.contains("return type mismatch"), "{e}");
    let e2 = check("fn g() { return 1; }").unwrap_err();
    assert!(e2.contains("void function cannot return"), "{e2}");
    // bare return in void fn is fine
    assert!(check("fn g() { return; }").is_ok());
}

#[test]
fn missing_return_detected() {
    let e = check("fn f(a: int) -> int { if a > 0 { return 1; } }").unwrap_err();
    assert!(e.contains("no return statement"), "{e}");
    // but both branches returning is fine
    assert!(check(
        "fn f(a: int) -> int { if a > 0 { return 1; } else { return 2; } }"
    )
    .is_ok());
}

#[test]
fn shadowing_allowed() {
    assert!(check(
        "fn main() -> int {
            x := 1;
            if true { x := \"now a string\"; }
            return 0;
        }"
    )
    .is_ok());
}

#[test]
fn duplicate_function() {
    let e = check("fn f() -> int { return 0; } fn f() -> int { return 1; }").unwrap_err();
    assert!(e.contains("duplicate function"), "{e}");
}

#[test]
fn forward_reference_ok() {
    assert!(check(
        "fn main() -> int { return later(2); }
         fn later(x: int) -> int { return x * 10; }"
    )
    .is_ok());
}
