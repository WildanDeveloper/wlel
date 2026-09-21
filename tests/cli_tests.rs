//! End-to-end CLI tests: `wlel test/run/check` on real files through the
//! real compiler pipeline (needs a C compiler on PATH, like `wlel` itself).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct TempWl {
    path: PathBuf,
}

impl TempWl {
    /// write a unique temp .wl file with the given source
    fn new(name: &str, src: &str) -> TempWl {
        let dir = std::env::temp_dir().join(format!(
            "wlel_cli_{}_{}",
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
fn test_runner_all_pass() {
    let t = TempWl::new(
        "all_pass",
        r#"fn add(a: int, b: int) -> int { return a + b; }
           test "arithmetic" {
               assert_eq(add(20, 22), 42);
           }
           test "string" {
               assert_eq("he" + "llo", "hello");
           }"#,
    );
    let (out, err, code) = t.run("test");
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("pass: arithmetic"), "{out}");
    assert!(out.contains("pass: string"), "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
}

#[test]
fn test_runner_reports_failure_with_file_line() {
    let t = TempWl::new(
        "one_fail",
        r#"test "ok" {
               assert(true);
           }
           test "kaput" {
               assert_eq(1, 2);
           }
           test "still runs" {
               assert_eq(3, 3);
           }"#,
    );
    let (out, _err, code) = t.run("test");
    assert_eq!(code, Some(1), "stdout: {out}");
    assert!(out.contains("pass: ok"), "{out}");
    assert!(out.contains("FAIL: kaput ("), "{out}");
    // failure carries the source location of the assert
    assert!(out.contains("one_fail.wl:5"), "{out}");
    // a failed test does not stop the suite
    assert!(out.contains("pass: still runs"), "{out}");
    assert!(out.contains("2 passed, 1 failed"), "{out}");
}

#[test]
fn run_mode_ignores_tests() {
    let t = TempWl::new(
        "run_ignores",
        r#"use std;
           test "not executed" {
               assert(false);
           }
           fn main() -> int {
               std::println_str("from main");
               return 0;
           }"#,
    );
    let (out, _err, code) = t.run("run");
    assert_eq!(code, Some(0), "stdout: {out}");
    assert!(out.contains("from main"), "{out}");
    assert!(!out.contains("pass:"), "{out}");
    assert!(!out.contains("FAIL"), "{out}");
}

#[test]
fn test_mode_reports_syntax_errors() {
    let t = TempWl::new(
        "broken",
        r#"test "x" {
               x := ;
           }"#,
    );
    let (out, err, code) = t.run("test");
    assert_ne!(code, Some(0), "stdout: {out}");
    assert!(err.contains("syntax error"), "{err}");
}

#[test]
fn tests_from_imported_files_run_too() {
    // main.wl imports lib/geom.wl; geom's tests are part of the suite
    let t = TempWl::new(
        "with_import",
        r#"use "lib/geom.wl";
           fn main() -> int { return 0; }"#,
    );
    let lib_dir = t.path.parent().unwrap().join("lib");
    fs::create_dir_all(&lib_dir).expect("mkdir lib");
    fs::write(
        lib_dir.join("geom.wl"),
        r#"fn twice(x: int) -> int { return x * 2; }
           test "menggandakan" {
               assert_eq(twice(21), 42);
           }"#,
    )
    .expect("write geom.wl");
    let (out, err, code) = t.run("test");
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("pass: menggandakan"), "{out}");
    assert!(out.contains("1 passed, 0 failed"), "{out}");
}

#[test]
fn dev_mode_bounds_read_reports_file_line() {
    let t = TempWl::new(
        "dev_bounds",
        r#"fn main() -> int {
               a := [10, 20, 30];
               return a[7];
           }"#,
    );
    let (_out, err, code) = t.run("run");
    assert_eq!(code, Some(1), "stderr: {err}");
    assert!(err.contains("bounds check failed"), "{err}");
    // the failure points back into the .wl source, not the generated C
    assert!(err.contains("dev_bounds.wl:3"), "{err}");
    assert!(err.contains("index 7, length 3"), "{err}");
}

#[test]
fn dev_mode_bounds_write_reports_file_line() {
    let t = TempWl::new(
        "dev_write",
        r#"fn main() -> int {
               a := [1, 2, 3];
               a[5] = 9;
               return 0;
           }"#,
    );
    let (_out, err, code) = t.run("run");
    assert_eq!(code, Some(1), "stderr: {err}");
    assert!(err.contains("bounds check failed"), "{err}");
    assert!(err.contains("dev_write.wl:3"), "{err}");
}

#[test]
fn dev_mode_div_zero_reports_file_line() {
    let t = TempWl::new(
        "dev_divz",
        r#"fn main() -> int {
               z := 0;
               return 10 / z;
           }"#,
    );
    let (_out, err, code) = t.run("run");
    assert_eq!(code, Some(1), "stderr: {err}");
    assert!(err.contains("division by zero"), "{err}");
    assert!(err.contains("dev_divz.wl:3"), "{err}");
}

#[test]
fn dev_mode_keeps_valid_programs_working() {
    let t = TempWl::new(
        "dev_ok",
        r#"use std;
           fn main() -> int {
               let a: [int; 4] = [2, 4, 6, 8];
               heap := new(int, 3);
               heap[2] = 5;
               f := 1.0;
               total := a[0] + a[3] + heap[2] + 8 / 2;
               std::println_int(total);
               std::println_float(f / 2.0);
               return 0;
           }"#,
    );
    let (out, err, code) = t.run("run");
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("19"), "{out}");
    assert!(out.contains("0.5"), "{out}");
}

#[test]
fn release_build_has_zero_checks() {
    let t = TempWl::new(
        "rel_build",
        r#"fn main() -> int {
               a := [1, 2, 3];
               z := 2;
               return a[1] + 10 / z;
           }"#,
    );
    let out_bin = t.path.with_extension("bin");
    let out_c = t.path.with_extension("c");
    let st = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", t.path.to_str().unwrap(), "-o", out_bin.to_str().unwrap(), "--emit-c"])
        .output()
        .expect("spawn wlel build");
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let c = fs::read_to_string(&out_c).expect("read emitted c");
    assert!(!c.contains("_wlel_arr_at"), "release C must have no bounds checks");
    assert!(!c.contains("_wlel_divz"), "release C must have no div-zero checks");
    assert!(!c.contains("0xDE"), "release C must have no arena poison");
    let run = Command::new(&out_bin).output().expect("run release binary");
    // a[1] + 10 / z = 2 + 5 = 7 — full result, no instrumentation involved
    assert_eq!(run.status.code(), Some(7));
}

// these two require ASan: MinGW-w64 GCC on Windows has no AddressSanitizer,
// so they run on the Linux/macOS CI legs only
#[cfg(unix)]
#[test]
fn build_sanitize_runs_clean_program() {
    let t = TempWl::new(
        "sanitize_ok",
        r#"use std;
           fn main() -> int {
               arena(4096) {
                   p := new(int, 32);
                   p[31] = 5;
                   std::println_int(p[31]);
               }
               return 0;
           }"#,
    );
    let bin = t.path.with_extension("bin");
    let st = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", t.path.to_str().unwrap(), "-o", bin.to_str().unwrap(), "-sanitize"])
        .output()
        .expect("spawn wlel build");
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let run = Command::new(&bin).output().expect("run sanitized binary");
    assert_eq!(run.status.code(), Some(0), "{}", String::from_utf8_lossy(&run.stderr));
    // a sanitized build still passes LeakSanitizer at exit (root arena)
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(!err.contains("AddressSanitizer"), "{err}");
    assert!(!err.contains("LeakSanitizer"), "{err}");
}

#[cfg(unix)]
#[test]
fn build_sanitize_catches_heap_overflow() {
    let t = TempWl::new(
        "sanitize_oob",
        r#"fn main() -> int {
               p := wlel_alloc(12) as *int;
               i := sys::argc() + 8;
               return p[i];
           }"#,
    );
    let bin = t.path.with_extension("bin");
    let st = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", t.path.to_str().unwrap(), "-o", bin.to_str().unwrap(), "-sanitize"])
        .output()
        .expect("spawn wlel build");
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let run = Command::new(&bin).output().expect("run sanitized binary");
    // release codegen has no bounds checks, so ASan itself must abort on
    // the out-of-bounds read; wlel_alloc is raw malloc (redzoned) and the
    // index comes from sys::argc() so the optimizer cannot fold the access
    assert_ne!(run.status.code(), Some(0), "ASan must abort");
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(err.contains("AddressSanitizer"), "{err}");
    assert!(err.contains("heap-buffer-overflow"), "{err}");
}
