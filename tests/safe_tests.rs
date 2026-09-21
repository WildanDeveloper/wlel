//! Safe-debug mode verification: the dev C (bounds checks, div-zero
//! checks, arena poison-fill) must compile and run clean under
//! AddressSanitizer + UBSan, exactly like the release C.

use std::fs;
use std::process::Command;
use wlel::checker::Checker;
use wlel::codegen::{gen_program, gen_program_safe};
use wlel::lexer::Lexer;
use wlel::parser::Parser;

/// single-file front-end: parse, check, transpile
fn front(src: &str, safe: bool) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    Checker::check(&mut p).expect("typecheck");
    p.funcs[0].file = "safe_asan.wl".into();
    if safe {
        gen_program_safe(&p)
    } else {
        gen_program(&p)
    }
}

/// Sanitizer flags this toolchain can actually link, probed once: full
/// ASan+UBSan where available (Linux/macOS CI). MinGW-w64 GCC on Windows
/// ships neither an ASan nor a UBSan runtime, so the probe degrades to ""
/// there — meaning "no sanitizer instrumentation"; the tests below still
/// verify wlel's own failure paths (clean exit codes, file:line messages),
/// only the sanitizer-output assertions become vacuous.
fn sanitizer_flags() -> &'static str {
    static FLAGS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    FLAGS.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("wlel_probe_{}", std::process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let c_path = dir.join("probe.c");
        let bin = dir.join("probe.bin");
        fs::write(&c_path, "int main(void) { return 0; }\n").expect("write probe");
        let cc = ["gcc", "cc", "clang"]
            .iter()
            .find(|cc| Command::new(cc).arg("--version").output().is_ok())
            .copied()
            .expect("need a C compiler");
        for flags in ["-fsanitize=address,undefined", "-fsanitize=undefined", ""] {
            let mut cmd = Command::new(cc);
            cmd.arg("-O0");
            if !flags.is_empty() {
                cmd.arg(flags);
            }
            cmd.arg("-o").arg(&bin).arg(&c_path);
            let ok = cmd.output().map(|o| o.status.success()).unwrap_or(false);
            if ok {
                return flags.to_string();
            }
        }
        String::new()
    })
}

/// compile with the best available sanitizers (possibly none) and run;
/// returns (exit code, combined output)
fn cc_asan_run(c: &str, tag: &str) -> (Option<i32>, String) {
    let dir = std::env::temp_dir().join(format!("wlel_safe_{}", std::process::id()));
    fs::create_dir_all(&dir).expect("mkdir");
    let c_path = dir.join(format!("{tag}.c"));
    let bin = dir.join(format!("{tag}.bin"));
    fs::write(&c_path, c).expect("write c");
    let cc = ["gcc", "cc", "clang"]
        .iter()
        .find(|cc| Command::new(cc).arg("--version").output().is_ok())
        .copied()
        .expect("need a C compiler");
    let mut cmd = Command::new(cc);
    cmd.args(["-O1", "-g", "-fwrapv"]);
    let flags = sanitizer_flags();
    if !flags.is_empty() {
        cmd.arg(flags);
    }
    cmd.args(["-o"]).arg(&bin).arg(&c_path);
    let st = cmd.output().expect("spawn cc");
    assert!(
        st.status.success(),
        "cc failed: {}",
        String::from_utf8_lossy(&st.stderr)
    );
    let run = Command::new(&bin).output().expect("run binary");
    let out = format!(
        "{}{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    (run.status.code(), out)
}

const ARENA_PROGRAM: &str = r#"use std;
struct Pt { x: int, y: int }

fn main() -> int {
    let a: [int; 6] = [1, 2, 3, 4, 5, 6];
    t := 0;
    for i in 0..6 { t += a[i]; }
    std::println_int(t);
    std::println_int(84 / 2);
    arena(1 << 16) {
        pts := new(Pt, 64);
        pts[63].x = 40;
        pts[63].y = 2;
        std::println_int(pts[63].x + pts[63].y);
        s := "safe-" + "mode";
        std::println_str(s);
    }
    return 0;
}"#;

#[test]
fn safe_dev_c_is_asan_clean() {
    let (code, out) = cc_asan_run(&front(ARENA_PROGRAM, true), "dev");
    assert_eq!(code, Some(0), "{out}");
    assert!(!out.contains("AddressSanitizer"), "{out}");
    assert!(!out.contains("runtime error"), "{out}");
    assert!(out.contains("21"), "{out}");
    assert!(out.contains("42"), "{out}");
    assert!(out.contains("safe-mode"), "{out}");
}

#[test]
fn release_c_is_asan_clean() {
    let (code, out) = cc_asan_run(&front(ARENA_PROGRAM, false), "rel");
    assert_eq!(code, Some(0), "{out}");
    assert!(!out.contains("AddressSanitizer"), "{out}");
    assert!(!out.contains("runtime error"), "{out}");
}

#[test]
fn bounds_failure_exits_cleanly_under_asan() {
    let src = r#"fn main() -> int {
    a := [1, 2, 3];
    return a[9];
}"#;
    let (code, out) = cc_asan_run(&front(src, true), "oob");
    // our own clean exit(1), not an ASan report or crash signal
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("bounds check failed at safe_asan.wl:3"), "{out}");
    assert!(!out.contains("AddressSanitizer"), "{out}");
}

#[test]
fn div_zero_failure_exits_cleanly_under_asan() {
    let src = r#"fn main() -> int {
    z := 0;
    return 5 % z;
}"#;
    let (code, out) = cc_asan_run(&front(src, true), "divz");
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("division by zero at safe_asan.wl:3"), "{out}");
    assert!(!out.contains("AddressSanitizer"), "{out}");
}
