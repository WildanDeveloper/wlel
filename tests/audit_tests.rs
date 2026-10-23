//! Regression tests for the full-source bug audit: compiler crashes and
//! undefined behavior that the earlier suites did not cover — runaway
//! monomorphization, deep expression chains, infinitely-sized structs,
//! signed division/shift overflow, safe-mode width preservation, manifest
//! validation and git argument injection.

use std::fs;
use std::process::Command;

use wlel::checker::Checker;
use wlel::lexer::Lexer;
use wlel::parser::Parser;

fn check_err(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    Checker::check(&mut p).expect_err("must be rejected").msg
}

fn check_ok(src: &str) {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    Checker::check(&mut p).expect("must type-check");
}

/// run a source through `wlel run` (safe mode), return (ok, stdout, stderr)
fn run_cli(name: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_audit_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join(format!("{name}.wlel"));
    fs::write(&path, src).expect("write source");
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    let _ = fs::remove_dir_all(&dir);
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// build (release mode) and run, returning (exit code, stdout, stderr)
fn build_run(name: &str, src: &str) -> (i32, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_audit_b_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let src_path = dir.join(format!("{name}.wlel"));
    let bin = dir.join("bin");
    fs::write(&src_path, src).expect("write source");
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", src_path.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output()
        .expect("spawn wlel build");
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        let _ = fs::remove_dir_all(&dir);
        return (-1, String::new(), err);
    }
    let run = Command::new(&bin).output().expect("run binary");
    let res = (
        run.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    );
    let _ = fs::remove_dir_all(&dir);
    res
}

// ---------------------------------------------------------------------------
// compiler robustness

#[test]
fn a_long_expression_chain_is_rejected_not_a_stack_overflow() {
    // a left-associative chain parses iteratively but is walked recursively;
    // 2000 operands used to abort the process with a stack overflow
    let src = format!("fn main() -> int {{ return {}; }}", vec!["1"; 3000].join(" + "));
    let msg = check_err(&src);
    assert!(msg.contains("expression nested too deeply"), "{msg}");

    // the limit is generous enough for real code
    let ok = format!("fn main() -> int {{ return {}; }}", vec!["1"; 200].join(" + "));
    check_ok(&ok);
}

#[test]
fn a_long_field_chain_is_rejected_not_a_stack_overflow() {
    let mut src = String::from("struct N { next: *N }\nfn main() -> int { p := new(N); return ");
    src.push('p');
    for _ in 0..3000 {
        src.push_str(".next");
    }
    src.push_str(" as int; }");
    let msg = check_err(&src);
    assert!(msg.contains("expression nested too deeply"), "{msg}");
}

#[test]
fn a_generic_that_grows_its_type_is_rejected_not_an_oom() {
    // f(Box { v: x }) instantiates f with a strictly larger type each time:
    // the compiler used to enqueue bodies until the machine ran out of memory
    let msg = check_err(
        "struct Box[T] { v: T }
         fn f[T](x: T) -> T { return f(Box { v: x }).v; }
         fn main() -> int { return f(1); }",
    );
    assert!(msg.contains("unbounded number of generic instances"), "{msg}");
}

#[test]
fn genuinely_recursive_generics_still_work() {
    // recursion at the *same* instantiation is fine (one instance)
    check_ok(
        "fn g[T](x: T, n: int) -> T {
             if n == 0 { return x; }
             return g(x, n - 1);
         }
         fn main() -> int { return g(7, 3); }",
    );
}

#[test]
fn a_struct_that_stores_itself_by_value_is_rejected() {
    // used to pass the checker and fail inside cc with "field has incomplete
    // type", pointing at a temp .c that had already been deleted
    let msg = check_err("struct N { next: N }\nfn main() -> int { return 0; }");
    assert!(msg.contains("contains itself by value"), "{msg}");
    assert!(msg.contains("store a pointer"), "must suggest the pointer fix: {msg}");

    let msg = check_err(
        "struct A { b: B }
         struct B { a: A }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("contains itself by value"), "{msg}");
    assert!(msg.contains("A -> B -> A"), "must name the cycle: {msg}");

    let msg = check_err("struct S { xs: [S; 2] }\nfn main() -> int { return 0; }");
    assert!(msg.contains("contains itself by value"), "{msg}");

    // a pointer or an array of pointers breaks the cycle
    check_ok("struct N { next: *N }\nfn main() -> int { return 0; }");
    check_ok("struct N { next: *N, prev: *N }\nfn main() -> int { return 0; }");

    // enums store their payloads inline too
    let msg = check_err("enum E { A(E) }\nfn main() -> int { return 0; }");
    assert!(msg.contains("contains itself by value"), "{msg}");
    let msg = check_err(
        "struct S { e: E }
         enum E { A(S) }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("E -> S -> E"), "must name the cycle: {msg}");
    check_ok("struct N { v: int }\nenum E { A(*N) }\nfn main() -> int { return 0; }");
}

// ---------------------------------------------------------------------------
// defined arithmetic (§11): no trap, no undefined behavior

#[test]
fn min_div_minus_one_wraps_instead_of_trapping() {
    // LLONG_MIN / -1 is UB in C and raises SIGFPE on x86; §11 defines signed
    // overflow as two's-complement wrap
    let src = "use std;
        fn main() -> int {
            let a: int = -9223372036854775807 - 1;
            std::println_int(a / -1);
            std::println_int(a % -1);
            return 0;
        }";
    let (ok, out, err) = run_cli("mindiv", src);
    assert!(ok, "safe mode must not die by signal: {err}");
    assert_eq!(out, "-9223372036854775808\n0\n", "stderr: {err}");

    let (rc, out, err) = build_run("mindiv_rel", src);
    assert_eq!(rc, 0, "release mode must not trap: {err}");
    assert_eq!(out, "-9223372036854775808\n0\n");
}

#[test]
fn compound_division_also_wraps_min_by_minus_one() {
    let src = "use std;
        fn main() -> int {
            let a: int = -9223372036854775807 - 1;
            a /= -1;
            let b: int = -9223372036854775807 - 1;
            b %= -1;
            std::println_int(a);
            std::println_int(b);
            return 0;
        }";
    let (ok, out, err) = run_cli("mindiv_c", src);
    assert!(ok, "{err}");
    assert_eq!(out, "-9223372036854775808\n0\n", "stderr: {err}");
}

#[test]
fn shift_counts_are_defined_out_of_range() {
    // `x << 64` / `x << -1` are UB in C; Wlel masks the count to the operand
    // width, in both build modes and on both backends
    let src = "use std;
        fn main() -> int {
            one := sys::argc() as int;      // 1
            std::println_int(one << (one + 63));   // 64 -> 0
            std::println_int(one << (one - 2));    // -1 -> 63
            return 0;
        }";
    let (ok, out, err) = run_cli("shift", src);
    assert!(ok, "{err}");
    assert_eq!(out, "1\n-9223372036854775808\n", "stderr: {err}");
}

#[test]
fn safe_mode_arithmetic_keeps_the_operand_width() {
    // a safe-mode `%` used to be wrapped in a `long long` helper, widening a
    // u32 expression to 64 bits and changing subtraction that wraps at 32
    let src = "use std;
        fn main() -> int {
            let a: u32 = 964u32 % 75u32;
            let b: u32 = 669u32 - (779u32 | a);
            std::println_int(b as int);
            return 0;
        }";
    let (ok, out, err) = run_cli("width", src);
    assert!(ok, "{err}");
    assert_eq!(out, "4294967122\n", "stderr: {err}");
}

// ---------------------------------------------------------------------------
// diagnostics

#[test]
fn a_lexer_error_reports_its_position_once() {
    let dir = std::env::temp_dir().join(format!("wlel_audit_lex_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("bad.wlel");
    fs::write(&path, "fn main() -> int { return 99999999999999999999; }\n").expect("write");
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["check", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    let err = String::from_utf8_lossy(&out.stderr);
    let _ = fs::remove_dir_all(&dir);
    assert!(err.contains("bad.wlel:"), "{err}");
    // `file:1:27: message`, not `file:1:27: 1:27: message`
    assert_eq!(err.matches("1:27").count(), 1, "duplicated span: {err}");
}

// ---------------------------------------------------------------------------
// project manifests and git dependencies are untrusted input

fn project_check(manifest: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "wlel_audit_man_{}_{}",
        std::process::id(),
        manifest.len()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).expect("mkdir");
    fs::write(dir.join("wlel.toml"), manifest).expect("write manifest");
    fs::write(dir.join("src/main.wlel"), "fn main() -> int { return 0; }\n").expect("write");
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["check"])
        .current_dir(&dir)
        .output()
        .expect("spawn wlel");
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    let _ = fs::remove_dir_all(&dir);
    err
}

#[test]
fn a_package_name_cannot_escape_the_project_directory() {
    // the package name becomes the default binary name; `../evil` used to be
    // accepted and `wlel build` wrote outside the project
    let err = project_check("[package]\nname = \"../evil\"\nversion = \"1.0.0\"\n");
    assert!(err.contains("invalid package name"), "{err}");
    let err = project_check("[package]\nname = \"a b\"\nversion = \"1.0.0\"\n");
    assert!(err.contains("invalid package name"), "{err}");
}

#[test]
fn a_manifest_version_must_parse() {
    let err = project_check("[package]\nname = \"x\"\nversion = \"not-a-version\"\n");
    assert!(err.contains("invalid package version"), "{err}");
}

#[test]
fn an_inline_table_rejects_duplicates_and_trailing_text() {
    let err = project_check(
        "[package]\nname = \"x\"\nversion = \"1.0.0\"\n[deps]\ny = { path = \"a\", path = \"b\" }\n",
    );
    assert!(err.contains("duplicate key 'path'"), "{err}");
    let err = project_check(
        "[package]\nname = \"x\"\nversion = \"1.0.0\"\n[deps]\ny = { path = \"a\" } z = { path = \"b\" }\n",
    );
    assert!(err.contains("trailing text after the inline table"), "{err}");
}

#[test]
fn a_git_url_cannot_smuggle_a_git_option() {
    // `git = "--upload-pack=..."` used to be passed straight to git, which
    // parses a leading dash as an option rather than a repository
    let err = project_check(
        "[package]\nname = \"x\"\nversion = \"1.0.0\"\n[deps]\ny = { git = \"--upload-pack=touch /tmp/wlel_pwn\" }\n",
    );
    assert!(err.contains("must not start with '-'"), "{err}");
}

// ---------------------------------------------------------------------------
// lexer diagnostics

#[test]
fn an_invalid_radix_digit_says_so_instead_of_out_of_range() {
    // a lexer error surfaces before the parser runs, so assert on the
    // message the lexer produces
    let e = Lexer::new("fn main() -> int { let x: int = 0b102; return 0; }")
        .tokenize()
        .expect_err("0b102 is not a binary literal");
    assert!(e.msg.contains("invalid digit for a base-2 literal"), "{}", e.msg);
    let e = Lexer::new("fn main() -> int { let x: int = 0o8; return 0; }")
        .tokenize()
        .expect_err("0o8 is not an octal literal");
    assert!(e.msg.contains("invalid digit for a base-8 literal"), "{}", e.msg);
    // a genuine overflow still reads as one, and quotes what the user wrote
    let e = Lexer::new("fn main() -> int { let x: int = 0xFFFFFFFFFFFFFFFFFFFF; return 0; }")
        .tokenize()
        .expect_err("out of range");
    assert!(e.msg.contains("out of range"), "{}", e.msg);
    assert!(e.msg.contains("0xFFFF"), "must echo the source spelling: {}", e.msg);
}

// ---------------------------------------------------------------------------
// cache and scratch files are private to the user

#[test]
fn the_build_cache_is_private_and_outside_tmp() {
    // a world-writable /tmp/wlel_cache let another local user pre-plant a
    // binary at a content-hash path that `wlel run` would execute
    let dir = std::env::temp_dir().join(format!("wlel_audit_cache_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let src = dir.join("m.wlel");
    fs::write(&src, "fn main() -> int { return 0; }\n").expect("write");
    let cache = dir.join("cache");
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .env("WLEL_CACHE_DIR", &cache)
        .args(["run", src.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(cache.is_dir(), "WLEL_CACHE_DIR must be honoured");
    // no scratch files left behind
    let leftovers: Vec<String> = fs::read_dir(&cache)
        .expect("read cache")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("scratch-"))
        .collect();
    assert!(leftovers.is_empty(), "scratch files left: {leftovers:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_generic_struct_cycle_is_caught_after_monomorphization() {
    // `S[T] { me: S[int] }` looks acyclic in the source; the cycle only
    // appears once S[int] exists, and used to reach cc as
    // "field 'me' has incomplete type"
    let msg = check_err(
        "struct S[T] { v: T, me: S[int] }
         fn main() -> int { p := new(S[int]); return 0; }",
    );
    assert!(msg.contains("contains itself by value"), "{msg}");
    // reported under the source spelling, not the mangled C name
    assert!(msg.contains("S[int]"), "must use the source type name: {msg}");

    let msg = check_err(
        "struct A[T] { b: B[T] }
         struct B[T] { a: A[T] }
         fn main() -> int { p := new(A[int]); return 0; }",
    );
    assert!(msg.contains("A[int] -> B[int] -> A[int]"), "{msg}");

    // a pointer field keeps the generic recursion legal
    check_ok(
        "struct S[T] { v: T, next: *S[T] }
         fn main() -> int { p := new(S[int]); return 0; }",
    );
}

#[test]
fn a_source_name_cannot_impersonate_a_monomorphized_type() {
    // `Vec[int]` is emitted as `Vec__int`; a user struct with that spelling
    // used to shadow the generated instance and break the stdlib's methods
    let toks = Lexer::new("struct Vec__int { x: int }\nfn main() -> int { return 0; }")
        .tokenize()
        .expect("lex");
    let (_, errs) = Parser::new(&toks).program();
    assert!(!errs.is_empty(), "must be rejected");
    assert!(
        errs[0].msg.contains("reserved for generic instantiation"),
        "{}",
        errs[0].msg
    );

    // the stdlib's own desugared names (`Result__is_ok`) are generated, not
    // parsed, so the stdlib must keep working
    let toks = Lexer::new(
        "use std; fn main() -> int { let r: Result[int, string] = Ok(1); return r.unwrap(); }",
    )
    .tokenize()
    .expect("lex");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "{errs:?}");
    wlel::stdsrc::splice_std(&mut p);
    Checker::check(&mut p).expect("stdlib must keep type-checking");
}

#[test]
fn a_type_cannot_be_named_main() {
    // `struct main` type-checked but the emitted C then had a type and a
    // function with the same name (`int main(...)`), which cc rejected
    let msg = check_err("struct main { x: int }\nfn main() -> int { return 0; }");
    assert!(msg.contains("is reserved"), "{msg}");
    let msg = check_err("enum main { A }\nfn main() -> int { return 0; }");
    assert!(msg.contains("is reserved"), "{msg}");

    // a builtin type name can never be re-declared (the builtin always wins,
    // so the user type would be unreachable)
    for name in ["string", "u8", "float", "byte", "void", "usize"] {
        let msg = check_err(&format!(
            "struct {name} {{ x: int }}\nfn main() -> int {{ return 0; }}"
        ));
        assert!(msg.contains("is reserved"), "{name}: {msg}");
    }
}
