//! Host integration: the `sys::` functions a compiler (or any tool that
//! drives one) needs — run a process and capture its output, read the
//! environment, list a directory, read stdin, write stderr, and change the
//! filesystem.
//!
//! These are the capabilities that were missing before self-hosting, so each
//! test is an end-to-end `wlel run` of a real program rather than a unit test
//! of the checker.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

struct Case {
    dir: PathBuf,
}

impl Case {
    fn new(name: &str) -> Case {
        let dir = std::env::temp_dir().join(format!(
            "wlel_host_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        Case { dir }
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.dir.join(name), text).expect("write source");
    }

    /// run the program with optional stdin, returning (stdout, stderr, code)
    fn run_with(&self, name: &str, stdin: Option<&str>) -> (String, String, Option<i32>) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wlel"));
        cmd.args(["run", name])
            .current_dir(&self.dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = cmd.spawn().expect("spawn wlel");
        if let Some(text) = stdin {
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(text.as_bytes())
                .expect("write stdin");
        }
        let out = child.wait_with_output().expect("wait");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }

    fn run(&self, name: &str) -> (String, String, Option<i32>) {
        self.run_with(name, None)
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn spawn_captures_stdout_and_returns_the_exit_status() {
    let c = Case::new("spawn");
    c.write(
        "main.wlel",
        r#"use std;
fn main() -> int {
    out := "";
    code := sys::spawn("echo hello", &out);
    std::println_int(code);
    std::println_str(std::str_trim(out));
    bad := "";
    fail := sys::spawn("exit 3", &bad);
    std::println_int(fail);
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains('0'), "{out}");
    assert!(out.contains("hello"), "{out}");
    assert!(out.contains('3'), "{out}");
}

#[test]
fn run_returns_the_status_without_capturing() {
    let c = Case::new("run");
    c.write(
        "main.wlel",
        r#"use std;
fn main() -> int {
    std::println_int(sys::run("true"));
    std::println_int(sys::run("false"));
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, ["0", "1"], "{out}");
}

#[test]
fn a_program_can_drive_the_c_compiler() {
    // the exact shape of `wlel build`: write C, invoke cc, run the binary
    let c = Case::new("drive");
    c.write(
        "main.wlel",
        "use std;\n\nfn main() -> int {\n    dir := sys::getenv(\"WLEL_TEST_DIR\");\n    cfile := dir + \"/hello.c\";\n    exe := dir + \"/hello\";\n    src := \"#include <stdio.h>\\nint main(void) { printf(\\\"from C\\\\n\\\"); return 0; }\\n\";\n    if !sys::write_file(cfile, src) {\n        sys::eprintln(\"write failed\");\n        return 1;\n    }\n    log := \"\";\n    if sys::spawn(\"cc -O1 -o \" + exe + \" \" + cfile + \" 2>&1\", &log) != 0 {\n        sys::eprintln(log);\n        return 1;\n    }\n    stdout := \"\";\n    rc := sys::spawn(exe, &stdout);\n    std::print_str(stdout);\n    std::println_int(rc);\n    sys::remove(cfile);\n    sys::remove(exe);\n    return 0;\n}\n",
    );
    let dir = c.dir.display().to_string();
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", "main.wlel"])
        .current_dir(&c.dir)
        .env("WLEL_TEST_DIR", &dir)
        .output()
        .expect("spawn wlel");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("from C"), "{stdout}");
}

#[test]
fn getenv_reads_the_environment() {
    if std::env::var_os("HOME").is_none() {
        eprintln!("skipping: HOME is not set");
        return;
    }
    let c = Case::new("getenv");
    c.write(
        "main.wlel",
        r#"use std;
fn main() -> int {
    std::println_str(sys::getenv("HOME"));
    std::println_int(std::strlen(sys::getenv("WLEL_NO_SUCH_VAR_XYZ")));
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let home = std::env::var("HOME").expect("HOME");
    assert!(out.contains(&home), "{out}");
    assert!(out.lines().any(|l| l == "0"), "missing var is empty: {out}");
}

#[test]
fn mkdir_exists_rename_and_remove() {
    let c = Case::new("fsops");
    let dir = c.dir.display().to_string().replace('\\', "/");
    c.write(
        "main.wlel",
        &format!(
            r#"use std;
fn main() -> int {{
    a := "{dir}/made";
    b := "{dir}/moved";
    std::println_bool(sys::exists(a));
    std::println_bool(sys::mkdir(a));
    std::println_bool(sys::exists(a));
    std::println_bool(sys::rename(a, b));
    std::println_bool(sys::exists(a));
    std::println_bool(sys::remove(b));
    std::println_bool(sys::exists(b));
    return 0;
}}
"#
        ),
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, ["false", "true", "true", "true", "false", "true", "false"], "{out}");
}

#[test]
fn read_dir_lists_entries() {
    if cfg!(windows) {
        eprintln!("skipping: read_dir is a POSIX implementation");
        return;
    }
    let c = Case::new("readdir");
    fs::write(c.dir.join("one.txt"), "x").expect("write");
    fs::write(c.dir.join("two.txt"), "x").expect("write");
    let dir = c.dir.display().to_string().replace('\\', "/");
    c.write(
        "main.wlel",
        &format!(
            r#"use std;
fn main() -> int {{
    listing := sys::read_dir("{dir}");
    names := str_split(std::str_trim(listing), "\n");
    std::println_bool(names.len() >= 2);
    std::println_bool(std::str_find(listing, "one.txt") >= 0);
    std::println_bool(std::str_find(listing, "two.txt") >= 0);
    std::println_int(std::strlen(sys::read_dir("{dir}/does-not-exist")));
    return 0;
}}
"#
        ),
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("true"), "{out}");
    assert!(out.contains('0'), "missing dir is empty: {out}");
}

#[test]
fn stdin_is_readable_line_by_line_and_whole() {
    let c = Case::new("stdin");
    c.write(
        "main.wlel",
        r#"use std;
fn main() -> int {
    a := sys::read_line();
    b := sys::read_line();
    std::println_str(a);
    std::println_str(b);
    rest := sys::read_stdin();
    std::println_int(std::strlen(rest));
    return 0;
}
"#,
    );
    let (out, err, code) = c.run_with("main.wlel", Some("one\ntwo\nthree\n"));
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("one"), "{out}");
    assert!(out.contains("two"), "{out}");
    // "three\n" is what remains after the two lines
    assert!(out.lines().any(|l| l == "6"), "{out}");
}

#[test]
fn eprintln_goes_to_stderr_not_stdout() {
    let c = Case::new("stderr");
    c.write(
        "main.wlel",
        r#"use std;
fn main() -> int {
    sys::eprintln("diagnostic");
    std::println_str("result");
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("result"), "{out}");
    assert!(!out.contains("diagnostic"), "stdout must stay clean: {out}");
    assert!(err.contains("diagnostic"), "{err}");
}

#[test]
fn the_runtime_is_only_spliced_when_used() {
    // a program that never touches sys:: process helpers must not carry the
    // runtime (it would drag <dirent.h> and friends into every build)
    let c = Case::new("nosys");
    c.write(
        "main.wlel",
        "fn main() -> int { return 0; }\n",
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");

    c.write(
        "main.wlel",
        "use std;\nfn main() -> int { std::println_int(1); return 0; }\n",
    );
    let (out, err, code) = c.wlel_build("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let generated = fs::read_to_string(c.dir.join("main.c")).expect("generated C");
    assert!(
        !generated.contains("_wlel_readdir"),
        "the host runtime leaked into a program that does not use it"
    );
}

impl Case {
    fn wlel_build(&self, name: &str) -> (String, String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(["build", name, "-o", "main", "--emit-c"])
            .current_dir(&self.dir)
            .output()
            .expect("spawn wlel");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }
}
