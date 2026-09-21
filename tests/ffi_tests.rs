//! FFI (Fase 2): `extern fn` declarations bind to external C symbols;
//! `-l`/`-L` linker flags pass through to cc; Wlel structs are C structs
//! (layout verified with C11 static_assert against cc itself).

use std::fs;
use std::process::Command;

use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::parser::Parser;

/// single-file front-end: parse, check, transpile (no std)
fn front(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    Checker::check(&mut p).expect("typecheck");
    gen_program(&p)
}

fn parse_err(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (_, errs) = Parser::new(&toks).program();
    assert!(!errs.is_empty(), "expected a parse error");
    errs[0].msg.clone()
}

fn check_err(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    Checker::check(&mut p).expect_err("must be rejected").msg
}

// ---------------------------------------------------------------------------
// declarations & emitted C

#[test]
fn extern_prototype_is_public_and_bodyless() {
    let c = front(
        "extern fn foo(x: i32) -> f64;
         extern fn bar();
         fn main() -> int { return 0; }",
    );
    // non-static: resolves to the external symbol at link time
    assert!(c.contains("double foo(int32_t x);"), "{c}");
    assert!(c.contains("void bar(void);") || c.contains("void bar();"), "{c}");
    assert!(!c.contains("static double foo"), "{c}");
    // no definition is emitted for extern declarations
    assert!(!c.contains("foo(int32_t x) {"), "{c}");
    assert!(!c.contains("bar() {"), "{c}");
}

#[test]
fn extern_main_is_rejected() {
    let msg = check_err("extern fn main() -> int;");
    assert!(msg.contains("entry point 'main'"), "{msg}");
}

#[test]
fn extern_duplicate_rejected() {
    let msg = check_err("extern fn f() -> i32; fn f() -> int { return 0; }");
    assert!(msg.contains("duplicate function 'f'"), "{msg}");
    let msg = check_err("extern fn f() -> i32; extern fn f() -> i32;");
    assert!(msg.contains("duplicate function 'f'"), "{msg}");
}

#[test]
fn extern_generic_rejected() {
    let msg = parse_err("extern fn f[T](x: T) -> T;");
    assert!(msg.contains("extern functions cannot be generic"), "{msg}");
}

#[test]
fn extern_with_body_rejected() {
    let msg = parse_err("extern fn f() -> i32 { return 0; }");
    assert!(msg.contains("extern declaration takes no body"), "{msg}");
}

#[test]
fn extern_call_type_checking_applies() {
    // the signature is enforced exactly like a Wlel function's
    let msg = check_err(
        "extern fn foo(x: i32) -> i32;
         fn main() -> int { return foo(1.5); }",
    );
    assert!(msg.contains("foo"), "{msg}");
}

// ---------------------------------------------------------------------------
// struct layout is the C layout — verified by the C compiler itself

#[test]
fn struct_layout_matches_c_with_static_asserts() {
    let src = r#"
        struct Inner { a: f64, b: u8 }
        struct Outer { i: Inner, flag: bool, next: *Outer }
        fn main() -> int { return 0; }
    "#;
    let mut c = front(src);
    c.push_str(
        "_Static_assert(sizeof(struct Inner) == 16, \"Inner layout\");\n",
    );
    c.push_str(
        "_Static_assert(offsetof(struct Outer, flag) == 16, \"flag offset\");\n",
    );
    c.push_str(
        "_Static_assert(offsetof(struct Outer, next) == 24, \"next offset\");\n",
    );
    c.push_str(
        "_Static_assert(sizeof(struct Outer) == 32, \"Outer layout\");\n",
    );
    let dir = std::env::temp_dir().join(format!("wlel_ffi_{}", std::process::id()));
    fs::create_dir_all(&dir).expect("mkdir");
    let c_path = dir.join("layout.c");
    fs::write(&c_path, &c).expect("write c");
    let cc = ["cc", "gcc", "clang"]
        .iter()
        .find(|cc| Command::new(cc).arg("--version").output().is_ok())
        .copied()
        .expect("need a C compiler");
    let out = Command::new(cc)
        // no -std flag: this is exactly how wlel invokes cc (gnu17 default),
        // where the prelude's POSIX clock_gettime stays visible
        .arg("-fsyntax-only")
        .arg(&c_path)
        .output()
        .expect("spawn cc");
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "cc rejected the layout:\n{err}");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// end-to-end: the CLI links and runs real C library functions

#[test]
fn example_ffi_tests_pass() {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/ffi.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", example.to_str().unwrap(), "-l", "m"])
        .output()
        .expect("spawn wlel test");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

#[test]
fn build_with_link_flag_runs() {
    let dir = std::env::temp_dir().join(format!("wlel_ffi_cli_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("ffi.wl");
    fs::write(
        &path,
        r#"extern fn sqrt(x: f64) -> f64;
           fn main() -> int {
               wlel_print_float(sqrt(16.0));
               return 0;
           }"#,
    )
    .expect("write source");
    let bin = dir.join("ffi");
    let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", path.to_str().unwrap(), "-o", bin.to_str().unwrap(), "-l", "m"])
        .output()
        .expect("spawn wlel build");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&bin).output().expect("run binary");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "4");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bad_link_flag_is_a_usage_error() {
    let dir = std::env::temp_dir().join(format!("wlel_ffi_bad_{}", std::process::id()));
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("x.wl");
    fs::write(&path, "fn main() -> int { return 0; }").expect("write source");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", path.to_str().unwrap(), "-o", dir.join("x").to_str().unwrap(), "-l", "-O2"])
        .output()
        .expect("spawn wlel");
    assert!(!run.status.success(), "-l with a dashed value must fail");
    let _ = fs::remove_dir_all(&dir);
}
