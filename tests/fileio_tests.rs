//! File I/O (Fase 2): `sys::read_file`/`sys::write_file` whole-file helpers
//! and the defer-friendly `std::fs::open/read/write/close` handles built on
//! the reserved `File` struct (a wrapper of the C FILE*).

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
            f.file = "fio.wl".into();
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
            f.file = "fio.wl".into();
        }
    }
    Checker::check(&mut p).expect_err("must be rejected").msg
}

/// run a source file through `wlel run`, forwarding extra args to the program
fn run_cli(name: &str, src: &str, fwd: &[&str]) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_fio_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join(format!("{name}.wl"));
    fs::write(&path, src).expect("write source");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wlel"));
    cmd.arg("run").arg(&path);
    for a in fwd {
        cmd.arg(a);
    }
    let run = cmd.output().expect("spawn wlel");
    let out = (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
    );
    let _ = fs::remove_dir_all(&dir);
    out
}

// ---------------------------------------------------------------------------
// checking: helper emission, rewrites, reserved names

#[test]
fn helpers_emitted_and_calls_rewritten() {
    let c = front(
        "use std;
         fn main() -> int {
             content := \"\";
             if sys::read_file(\"x.txt\", &content) {
                 sys::write_file(\"y.txt\", content);
             }
             r := std::fs::open(\"x.txt\", \"rb\");
             match r {
                 Ok(f) => {
                     buf := new(u8, 8);
                     m := std::fs::read(f, buf, 8);
                     m = std::fs::write(f, buf, 8);
                     std::fs::close(f);
                 }
                 Err(_) => { }
             }
             return 0;
         }",
    )
    .expect("typecheck");
    // runtime helpers in the prelude
    assert!(c.contains("typedef struct File { void* h; } File;"), "{c}");
    assert!(c.contains("static _Bool _wlel_read_file(const char* path, const char** out)"), "{c}");
    assert!(c.contains("static _Bool _wlel_write_file(const char* path, const char* contents)"), "{c}");
    assert!(c.contains("static void _wlel_fs_close(File* f)"), "{c}");
    // the Result helper is spliced in only when called, and returns the
    // checker-registered enum instance by value
    assert!(c.contains("static Result__pFile__string _wlel_fs_open_r(const char* path, const char* mode)"), "{c}");
    assert!(c.contains("struct Result__pFile__string {"), "{c}");
    assert!(c.contains("_wlel_read_file(\"x.txt\", &content)"), "{c}");
    assert!(c.contains("_wlel_fs_open_r(\"x.txt\", \"rb\")"), "{c}");
    assert!(c.contains("_wlel_fs_read_r(f, buf, 8)"), "{c}");
    assert!(c.contains("_wlel_fs_close(f)"), "{c}");
    // a program that never touches std::fs must not carry the helpers
    let minimal = front("fn main() -> int { return 0; }").expect("typecheck");
    assert!(!minimal.contains("_wlel_fs_open_r"), "{minimal}");
}

#[test]
fn fs_requires_use_std_but_sys_does_not() {
    // sys:: is process-level: works without any import
    front("fn main() -> int { content := \"\"; if sys::read_file(\"x\", &content) { } return 0; }")
        .expect("sys::read_file needs no import");
    // std::fs:: is stdlib: gated behind `use std`
    let msg = check_err_msg(
        "fn main() -> int {
             f := std::fs::open(\"x\", \"r\");
             return 0;
         }",
    );
    assert!(msg.contains("'std::fs::open' requires `use std;`"), "{msg}");
}

#[test]
fn fs_unknown_function_rejected() {
    let msg = check_err_msg(
        "use std;
         fn main() -> int {
             std::fs::seek(0 as *File);
             return 0;
         }",
    );
    assert!(msg.contains("unknown std function 'std::fs::seek'"), "{msg}");
}

#[test]
fn fs_argument_type_errors() {
    let cases: &[(&str, &str)] = &[
        (
            "std::fs::open(\"x\", 5);",
            "std::fs::open: mode must be string",
        ),
        (
            "std::fs::read(\"x\", new(u8, 1), 1);",
            "std::fs::read: f must be *File",
        ),
        (
            "f := result_unwrap(std::fs::open(\"x\", \"r\"));
             std::fs::read(f, \"str\", 1);",
            "std::fs::read: buf must be *u8",
        ),
        (
            "f := result_unwrap(std::fs::open(\"x\", \"r\"));
             std::fs::read(f, new(u8, 1), \"n\");",
            "std::fs::read: n must be int",
        ),
        (
            "f := result_unwrap(std::fs::open(\"x\", \"r\"));
             std::fs::write(f, \"str\", 1);",
            "std::fs::write: buf must be *u8",
        ),
        (
            "std::fs::close(\"x\");",
            "std::fs::close: f must be *File",
        ),
        (
            "std::fs::read_all(5);",
            "std::fs::read_all: path must be string",
        ),
        (
            "std::fs::write_all(\"x\", 5);",
            "std::fs::write_all: contents must be string",
        ),
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
fn sys_file_argument_type_errors() {
    let msg = check_err_msg(
        "fn main() -> int { n := 0; return sys::read_file(\"x\", &n) as int; }",
    );
    assert!(msg.contains("sys::read_file: out must be *string"), "{msg}");
    let msg = check_err_msg("fn main() -> int { return sys::write_file(1, \"x\") as int; }");
    assert!(msg.contains("sys::write_file: path must be string"), "{msg}");
    let msg = check_err_msg("fn main() -> int { return sys::write_file(\"x\", 2) as int; }");
    assert!(msg.contains("sys::write_file: contents must be string"), "{msg}");
}

#[test]
fn file_struct_name_is_reserved() {
    let msg = check_err_msg("struct File { x: int } fn main() -> int { return 0; }");
    assert!(msg.contains("struct 'File' is reserved"), "{msg}");
    let msg = check_err_msg("fn f[File](x: File) -> int { return 0; } fn main() -> int { return 0; }");
    assert!(msg.contains("type parameter 'File' shadows a built-in type"), "{msg}");
}

// ---------------------------------------------------------------------------
// end-to-end behavior via the CLI

#[test]
fn write_read_roundtrip_end_to_end() {
    // cross-OS: never assume /tmp; the compiler supports forward slashes on
    // every platform, so normalize the temp path and splice it in
    let tmp = std::env::temp_dir().join(format!("wlel_fio_rt_{}.tmp", std::process::id()));
    let tmp = tmp.to_str().unwrap().replace('\\', "/");
    // reading /dev/null is a POSIX trick; skip that probe on Windows
    let devnull = if cfg!(unix) {
        "               empty := \"\";\n               if sys::read_file(\"/dev/null\", &empty) { std::println_str(\"empty:[\" + empty + \"]\"); }\n"
    } else {
        ""
    };
    let src = r#"use std;
           fn main() -> int {
               p := "$TMP";
               if !sys::write_file(p, "one\ntwo\n") { std::println_str("write fail"); return 1; }
               back := "";
               if !sys::read_file(p, &back) { std::println_str("read fail"); return 1; }
               std::println_str("[" + back + "]");
               if !sys::read_file("/no/such/fio-target", &back) { std::println_str("missing ok"); }
$DEV               return 0;
           }"#
    .replace("$TMP", &tmp)
    .replace("$DEV", devnull);
    let (ok, out, err) = run_cli("roundtrip", &src, &[]);
    assert!(ok, "{err}");
    assert!(out.contains("[one\ntwo\n]"), "{out}");
    assert!(out.contains("missing ok"), "{out}");
    if cfg!(unix) {
        assert!(out.contains("empty:[]"), "{out}");
    }
}

#[test]
fn cat_like_tool_reads_argv_file() {
    let dir = std::env::temp_dir().join(format!("wlel_fio_cat_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let data = dir.join("data.txt");
    fs::write(&data, "cat body\nline 2\n").expect("write data");
    let src = r#"use std;
        fn main() -> int {
            if sys::argc() < 2 {
                std::println_str("usage: fileio <file>");
                return 1;
            }
            path := sys::arg(1);
            content := "";
            if !sys::read_file(path, &content) {
                std::println_str("fileio: cannot read " + path);
                return 1;
            }
            std::print_str(content);
            return 0;
        }"#;
    let wl = dir.join("cat.wl");
    fs::write(&wl, src).expect("write source");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", wl.to_str().unwrap(), data.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(String::from_utf8_lossy(&run.stdout), "cat body\nline 2\n");
    // missing file: non-zero exit + message
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", wl.to_str().unwrap(), dir.join("nope.txt").to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(!run.status.success());
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("cannot read"), "{out}");
    // no args: usage on stdout, exit 1
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", wl.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stdout).contains("usage:"), "{}", String::from_utf8_lossy(&run.stdout));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fs_handles_stream_end_to_end() {
    let tmp = std::env::temp_dir().join(format!("wlel_fio_h_{}.tmp", std::process::id()));
    let tmp = tmp.to_str().unwrap().replace('\\', "/");
    let head = r#"use std;
           fn main() -> int {
               p := "$TMP";"#
    .replace("$TMP", &tmp);
    let src = format!(
        "{}\n{}",
        head,
        r#"               if !sys::write_file(p, "abcdefgh") { return 1; }
               f := result_unwrap(std::fs::open(p, "rb"));
               defer std::fs::close(f);
               buf := new(u8, 8);
               if result_unwrap(std::fs::read(f, buf, 4)) != 4 { return 1; }
               buf[4] = 0;
               std::println_str(buf as string);
               if result_unwrap(std::fs::read(f, buf, 99)) != 4 { return 1; }
               buf[4] = 0;
               std::println_str(buf as string);
               if result_unwrap(std::fs::read(f, buf, 4)) != 0 { std::println_str("expected eof"); return 1; }
               std::println_str("eof ok");
               if result_is_err(std::fs::read(f, buf, -1)) == false { return 1; }
               // append, then a fresh handle sees the grown file
               g := result_unwrap(std::fs::open(p, "ab"));
               if result_unwrap(std::fs::write(g, "XY" as *u8, 2)) != 2 { return 1; }
               std::fs::close(g);
               whole := "";
               if !sys::read_file(p, &whole) { return 1; }
               std::println_str(whole);
               std::fs::close(f);
               std::fs::close(f);
               std::println_str("done");
               return 0;
           }"#
    );
    let (ok, out, err) = run_cli("handles", &src, &[]);
    assert!(ok, "{err}");
    for expect in ["abcd\n", "efgh\n", "eof ok\n", "abcdefghXY\n", "done\n"] {
        assert!(out.contains(expect), "want {expect:?} in:\n{out}");
    }
}

#[test]
fn fs_open_missing_file_yields_err_end_to_end() {
    let (ok, out, err) = run_cli(
        "missing",
        r#"use std;
           fn main() -> int {
               r := std::fs::open("/no/such/fio-target", "rb");
               match r {
                   Ok(f) => {
                       std::fs::close(f);
                       return 1;
                   }
                   Err(e) => {
                       std::println_str("err: " + e);
                       return 0;
                   }
               }
           }"#,
        &[],
    );
    assert!(ok, "{err}");
    assert!(out.contains("err: "), "{out}");
}

// ---------------------------------------------------------------------------
// dogfood: the example's own test blocks must run green

#[test]
fn example_fileio_test_blocks_pass() {
    // run from a temp CWD: the example's test blocks write their scratch
    // file relative to the current directory (works on every OS)
    let dir = std::env::temp_dir().join(format!("wlel_fio_example_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/fileio.wl");
    let test = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", example.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .expect("spawn wlel");
    assert!(test.status.success(), "{}", String::from_utf8_lossy(&test.stderr));
    let out = String::from_utf8_lossy(&test.stdout);
    assert!(out.contains("5 passed, 0 failed"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// memory safety
// requires ASan: MinGW-w64 GCC on Windows has no AddressSanitizer, so this
// runs on the Linux/macOS CI legs only (UBSan still covers Windows elsewhere)

#[cfg(unix)]
#[test]
fn file_io_program_is_asan_clean() {
    let dir = std::env::temp_dir().join(format!("wlel_fioasan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let tmp = dir.join("asan.tmp").to_str().unwrap().replace('\\', "/");
    let path = dir.join("asan.wl");
    let src = r#"use std;
           fn main() -> int {
               arena(1 << 16) {
                   p := "$TMP";"#
    .replace("$TMP", &tmp);
    let src = format!(
        "{}\n{}",
        src,
        r#"                   if !sys::write_file(p, "asan arena file\n") { return 1; }
                   f := result_unwrap(std::fs::open(p, "rb"));
                   defer std::fs::close(f);
                   buf := new(u8, 64);
                   n := result_unwrap(std::fs::read(f, buf, 63));
                   if n <= 0 { return 1; }
                   buf[n] = 0;
                   s := buf as string;
                   if std::strlen(s) != n { return 1; }
                   whole := "";
                   if !sys::read_file(p, &whole) { return 1; }
                   if whole != s { return 1; }
                   // churn handles across many blocks: wrappers die with
                   // their arena, close runs through the defer stack
                   for i in 0..200 {
                       arena(256) {
                           g := result_unwrap(std::fs::open(p, "rb"));
                           defer std::fs::close(g);
                           m := result_unwrap(std::fs::read(g, buf, 4));
                           assert(m == 4);
                       }
                   }
               }
                return 0;
            }"#,
    );
    fs::write(&path, &src).expect("write source");
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

#[test]
fn release_build_has_no_check_overhead_for_file_calls() {
    let src = "use std;
               fn main() -> int {
                   content := \"\";
                   if sys::read_file(\"x\", &content) { }
                   r := std::fs::open(\"x\", \"rb\");
                   match r {
                       Ok(f) => { std::fs::close(f); }
                       Err(_) => { }
                   }
                   return 0;
               }";
    let toks = Lexer::new(src).tokenize().unwrap();
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "{errs:?}");
    splice_std(&mut p);
    for f in p.funcs.iter_mut() {
        if f.file.is_empty() {
            f.file = "fio.wl".into();
        }
    }
    Checker::check(&mut p).expect("typecheck");
    let release = gen_program(&p);
    // plain direct calls in release: no dev-mode checking anywhere
    assert!(release.contains("_wlel_read_file(\"x\", &content)"), "{release}");
    assert!(release.contains("_wlel_fs_open_r(\"x\", \"rb\")"), "{release}");
    assert!(!release.contains("_wlel_arr_at"), "release must be check-free: {release}");
    assert!(!release.contains("_wlel_divz"), "{release}");
}
