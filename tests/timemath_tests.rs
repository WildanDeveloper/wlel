//! Time, math and random (Fase 2): `sys::mono_ms`/`sys::unix_ms` wall
//! clocks, the `std::math::*` libm wrappers, and the `std::random::*`
//! xorshift64* PRNG — plus the mono_ms benchmark harness they enable.

use std::fs;
use std::process::Command;

use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::parser::Parser;
use wlel::stdsrc::splice_std;

/// full pipeline including the `use std` splice, exactly like the CLI
fn front(src: &str) -> Result<String, String> {
    let toks = Lexer::new(src).tokenize().map_err(|e| e.to_string())?;
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    splice_std(&mut p);
    for f in p.funcs.iter_mut() {
        if f.file.is_empty() {
            f.file = "tm.wl".into();
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
            f.file = "tm.wl".into();
        }
    }
    Checker::check(&mut p).expect_err("must be rejected").msg
}

/// run a source file through `wlel run`
fn run_cli(name: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_tm_{name}_{}", std::process::id()));
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
// checking: helper emission, call mangling, gating

#[test]
fn helpers_emitted_and_calls_mangled() {
    let c = front(
        "use std;
         fn main() -> int {
             t := sys::mono_ms();
             u := sys::unix_ms();
             s := std::math::sqrt(2.0);
             p := std::math::pow(2.0, 10.0);
             h := std::math::hypot(3.0, 4.0);
             std::random::seed(42);
             n := std::random::next();
             r := std::random::int(10);
             f := std::random::float();
             return (t + u + s + p + h + r + f) as int + n as int * 0;
         }",
    )
    .expect("typecheck");
    // time helpers in the prelude
    assert!(c.contains("static long long sys__mono_ms(void)"), "{c}");
    assert!(c.contains("static long long sys__unix_ms(void)"), "{c}");
    assert!(c.contains("clock_gettime(CLOCK_MONOTONIC, &ts);"), "{c}");
    assert!(c.contains("clock_gettime(CLOCK_REALTIME, &ts);"), "{c}");
    // math wrappers over libm
    assert!(c.contains("static double std__math__sqrt(double x) { return sqrt(x); }"), "{c}");
    assert!(c.contains("static double std__math__pow(double b, double e) { return pow(b, e); }"), "{c}");
    assert!(c.contains("static double std__math__hypot(double a, double b) { return hypot(a, b); }"), "{c}");
    // xorshift64* rng
    assert!(c.contains("static unsigned long long _wlel_rng_state = 0;"), "{c}");
    assert!(c.contains("static void std__random__seed(long long s)"), "{c}");
    assert!(c.contains("static unsigned long long std__random__next(void)"), "{c}");
    assert!(c.contains("static long long std__random__int(long long n)"), "{c}");
    assert!(c.contains("static double std__random__float(void)"), "{c}");
    // call sites mangled namespace -> C names
    assert!(c.contains("sys__mono_ms()"), "{c}");
    assert!(c.contains("sys__unix_ms()"), "{c}");
    assert!(c.contains("std__math__sqrt(2.0)"), "{c}");
    assert!(c.contains("std__random__seed(42)"), "{c}");
    assert!(c.contains("std__random__next()"), "{c}");
}

#[test]
fn sys_time_needs_no_import_but_math_random_do() {
    // sys:: is process-level: works without any import
    front("fn main() -> int { t := sys::mono_ms(); u := sys::unix_ms(); if t <= u { return 1; } return 0; }")
        .expect("sys:: time needs no import");
    // std::math:: and std::random:: are stdlib: gated behind `use std`
    let msg = check_err_msg("fn main() -> int { std::math::sqrt(1.0); return 0; }");
    assert!(msg.contains("'std::math::sqrt' requires `use std;`"), "{msg}");
    let msg = check_err_msg("fn main() -> int { std::random::next(); return 0; }");
    assert!(msg.contains("'std::random::next' requires `use std;`"), "{msg}");
}

#[test]
fn math_unknown_function_rejected() {
    let msg = check_err_msg(
        "use std;
         fn main() -> int { std::math::foo(1.0); return 0; }",
    );
    assert!(msg.contains("unknown std function 'std::math::foo'"), "{msg}");
}

#[test]
fn math_argument_type_and_arity_errors() {
    let cases: &[(&str, &str)] = &[
        ("std::math::sqrt(\"x\");", "std::math::sqrt: argument must be float (f64), got string"),
        ("std::math::sqrt(true);", "std::math::sqrt: argument must be float (f64), got bool"),
        ("std::math::sqrt();", "std::math::sqrt takes exactly 1 argument(s), got 0"),
        ("std::math::sqrt(1.0, 2.0);", "std::math::sqrt takes exactly 1 argument(s), got 2"),
        ("std::math::pow(1.0);", "std::math::pow takes exactly 2 argument(s), got 1"),
        ("std::math::atan2(1.0, \"x\");", "std::math::atan2: argument must be float (f64), got string"),
        ("p := 0 as *int; std::math::fabs(p);", "std::math::fabs: argument must be float (f64), got *int"),
    ];
    for (call, want) in cases {
        let src = format!(
            "use std;\n fn main() -> int {{\n {call}\n return 0;\n }}"
        );
        let msg = check_err_msg(&src);
        assert!(msg.contains(want), "want {want:?} got: {msg}");
    }
}

#[test]
fn math_f32_requires_explicit_cast_but_ints_widen() {
    // mixed-width rule: f32 must cast explicitly
    let msg = check_err_msg(
        "use std;
         fn main() -> int { v := 1.5f32; std::math::sqrt(v); return 0; }",
    );
    assert!(msg.contains("std::math::sqrt: argument must be float (f64), got f32"), "{msg}");
    // cast makes it fine, and plain ints widen per the usual rules
    front("use std; fn main() -> int { v := 1.5f32; assert_eq(std::math::sqrt(v as float), 1.0); s := std::math::sqrt(9); return s as int; }")
        .expect("cast f32 and widening int accepted");
}

#[test]
fn math_returns_float() {
    let msg = check_err_msg(
        "use std;
         fn main() -> int { s := std::math::sqrt(4.0); return s; }",
    );
    assert!(msg.contains("return type mismatch: expected int, found float"), "{msg}");
    front("use std; fn main() -> float { return std::math::pow(2.0, 3.0); }")
        .expect("float return accepted");
}

#[test]
fn random_api_type_errors() {
    let cases: &[(&str, &str)] = &[
        ("std::random::seed(\"x\");", "std::random::seed: s must be int, got string"),
        ("std::random::seed();", "std::random::seed(s) takes exactly 1 argument"),
        ("std::random::seed(1, 2);", "std::random::seed(s) takes exactly 1 argument"),
        ("std::random::next(1);", "std::random::next() takes no arguments"),
        ("std::random::int(\"x\");", "std::random::int: n must be int, got string"),
        ("std::random::int();", "std::random::int(n) takes exactly 1 argument"),
        ("std::random::float(1);", "std::random::float() takes no arguments"),
    ];
    for (call, want) in cases {
        let src = format!(
            "use std;\n fn main() -> int {{\n {call}\n return 0;\n }}"
        );
        let msg = check_err_msg(&src);
        assert!(msg.contains(want), "want {want:?} got: {msg}");
    }
    // unknown names are rejected with the namespace in the message
    let msg = check_err_msg("use std; fn main() -> int { std::random::bytes(1); return 0; }");
    assert!(msg.contains("unknown std function 'std::random::bytes'"), "{msg}");
}

#[test]
fn random_types_flow_through() {
    // next() is u64: it binds to a u64 annotation and compares against
    // u64 literals; int(n) is int, float() is float
    front(
        "use std;
         fn main() -> int {
             std::random::seed(1);
             let x: u64 = std::random::next();
             if x > 18446744073709551615u64 { return 1; }
             n := std::random::int(5);
             if n < 0 || n > 4 { return 1; }
             f := std::random::float();
             if f < 0.0 || f >= 1.0 { return 1; }
             return 0;
         }",
    )
    .expect("random types check");
}

// ---------------------------------------------------------------------------
// end-to-end behavior via the CLI

#[test]
fn seeded_random_is_reproducible_end_to_end() {
    let src = r#"use std;
        fn main() -> int {
            std::random::seed(42);
            for _i in 0..5 {
                std::println_str(std::format("{} {}", std::random::int(1000), std::random::float()));
            }
            return 0;
        }"#;
    let (ok1, out1, err1) = run_cli("repro1", src);
    let (ok2, out2, err2) = run_cli("repro2", src);
    assert!(ok1, "{err1}");
    assert!(ok2, "{err2}");
    assert!(!out1.is_empty());
    assert_eq!(out1, out2, "same seed must give the same stream");
    // a different seed gives a different stream
    let alt = src.replace("seed(42)", "seed(43)");
    let (ok3, out3, err3) = run_cli("repro3", &alt);
    assert!(ok3, "{err3}");
    assert_ne!(out1, out3, "different seeds should give different streams");
}

#[test]
fn unseeded_random_differs_between_processes_end_to_end() {
    let src = r#"use std;
        fn main() -> int {
            std::println_int(std::random::int(1000000000));
            return 0;
        }"#;
    let (ok1, out1, err1) = run_cli("unseeded1", src);
    let (ok2, out2, err2) = run_cli("unseeded2", src);
    assert!(ok1, "{err1}");
    assert!(ok2, "{err2}");
    assert_ne!(out1, out2, "clock-seeded processes should differ");
}

#[test]
fn mono_ms_times_a_loop_end_to_end() {
    let src = r#"use std;
        fn main() -> int {
            t0 := sys::mono_ms();
            s := 0;
            for _i in 0..3000000 { s += _i % 7; }
            t1 := sys::mono_ms();
            std::println_str(std::format("elapsed={} sink={}", t1 - t0, s));
            if t1 < t0 { std::println_str("monotonic clock went backwards"); return 1; }
            if t1 - t0 <= 0 { std::println_str("no measurable time passed"); return 1; }
            u0 := sys::unix_ms();
            if u0 < 1577836800000 { std::println_str("unix_ms implausibly small"); return 1; }
            std::println_str("clocks ok");
            return 0;
        }"#;
    let (ok, out, err) = run_cli("bench", src);
    assert!(ok, "{err}");
    assert!(out.contains("clocks ok"), "{out}");
    let line = out.lines().find(|l| l.starts_with("elapsed=")).expect("elapsed line");
    let ms: i64 = line["elapsed=".len()..].split(' ').next().expect("ms field").parse().expect("elapsed ms");
    assert!(ms > 0, "3M-iteration loop must take measurable time: {line}");
}

#[test]
fn math_values_end_to_end() {
    let src = r#"use std;
        fn main() -> int {
            if std::math::pow(2, 10) != 1024.0 { return 1; }
            if std::math::hypot(3, 4) != 5.0 { return 1; }
            if std::math::sqrt(4.0) != 2.0 { return 1; }
            if std::math::floor(2.7) != 2.0 { return 1; }
            if std::math::ceil(2.1) != 3.0 { return 1; }
            if std::math::round(2.5) != 3.0 { return 1; }
            if std::math::fmin(3.5, -1.0) != -1.0 { return 1; }
            if std::math::fmax(3.5, -1.0) != 3.5 { return 1; }
            if std::math::fabs(-7.0) != 7.0 { return 1; }
            tol := 0.0000000001;
            if std::math::fabs(std::math::sin(0.0)) > tol { return 1; }
            if std::math::fabs(std::math::cos(0.0) - 1.0) > tol { return 1; }
            if std::math::fabs(std::math::log(std::math::exp(2.0)) - 2.0) > tol { return 1; }
            if std::math::fabs(std::math::log10(1000.0) - 3.0) > tol { return 1; }
            if std::math::fabs(std::math::cbrt(64.0) - 4.0) > tol { return 1; }
            if std::math::fabs(std::math::atan2(1, 1) - 0.7853981633974483) > tol { return 1; }
            if std::math::fmod(7, 3) != 1.0 { return 1; }
            if std::math::tanh(0.0) != 0.0 { return 1; }
            std::println_str("math ok");
            return 0;
        }"#;
    let (ok, out, err) = run_cli("mathvals", src);
    assert!(ok, "{err}");
    assert!(out.contains("math ok"), "{out}");
}

// ---------------------------------------------------------------------------
// dogfood: the example's own test blocks must run green

#[test]
fn example_timemath_test_blocks_pass() {
    let test = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", "examples/timemath.wl"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawn wlel");
    assert!(test.status.success(), "{}", String::from_utf8_lossy(&test.stderr));
    let out = String::from_utf8_lossy(&test.stdout);
    assert!(out.contains("8 passed, 0 failed"), "{out}");
}

// ---------------------------------------------------------------------------
// memory safety

#[cfg(unix)]
#[test]
fn timemath_program_is_asan_clean() {
    let dir = std::env::temp_dir().join(format!("wlel_tmasan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("asan.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               arena(1 << 16) {
                   t0 := sys::mono_ms();
                   total := 0.0;
                   for _i in 0..1000 {
                       total += std::math::sin(std::random::float() * 3.14159);
                   }
                   std::random::seed(5);
                   for _i in 0..1000 {
                       assert(std::random::int(100) >= 0);
                   }
                   el := sys::mono_ms() - t0;
                   if total != total { return 1; }
                   if el < 0 { return 1; }
               }
               return 0;
           }"#,
    )
    .expect("write source");
    let bin = dir.join("asan");
    let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", path.to_str().unwrap(), "-o", bin.to_str().unwrap(), "-sanitize"])
        .output()
        .expect("spawn wlel");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&bin).output().expect("run sanitized binary");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// release = zero overhead

#[test]
fn release_build_has_no_check_overhead_for_timemath_calls() {
    let c = front(
        "use std;
         fn main() -> int {
             t := sys::mono_ms();
             s := std::math::sqrt(2.0);
             std::random::seed(1);
             r := std::random::int(10);
             return s as int + r + t as int * 0;
         }",
    )
    .expect("typecheck");
    // plain direct calls in release: no dev-mode checking anywhere
    assert!(c.contains("sys__mono_ms()"), "{c}");
    assert!(c.contains("std__math__sqrt(2.0)"), "{c}");
    assert!(!c.contains("_wlel_arr_at"), "release must be check-free: {c}");
    assert!(!c.contains("_wlel_divz"), "{c}");
}
