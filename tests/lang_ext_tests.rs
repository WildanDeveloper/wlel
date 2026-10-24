//! Language extensions for self-hosting: compile-time `const`, function
//! pointers, and an amortized string builder.
//!
//! Each test drives the real binary end to end, because these features are
//! about the emitted C as much as about the checker.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct Case {
    dir: PathBuf,
}

impl Case {
    fn new(name: &str) -> Case {
        let dir = std::env::temp_dir().join(format!(
            "wlel_ext_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        Case { dir }
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, text).expect("write source");
    }

    fn run(&self, name: &str) -> (String, String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(["run", name])
            .current_dir(&self.dir)
            .output()
            .expect("spawn wlel");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }

    fn check(&self, name: &str) -> (String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(["check", name])
            .current_dir(&self.dir)
            .output()
            .expect("spawn wlel");
        (
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
            out.status.code(),
        )
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// ---- const ----------------------------------------------------------------

#[test]
fn const_values_are_folded_and_usable() {
    let c = Case::new("const");
    c.write(
        "main.wlel",
        r#"use std;

const LIMIT: int = 10;
const DOUBLE: int = LIMIT * 2;
const NAME: string = "wlel";
const ON: bool = true;

fn main() -> int {
    std::println_int(LIMIT);
    std::println_int(DOUBLE);
    std::println_str(NAME);
    std::println_bool(ON);
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, ["10", "20", "wlel", "true"], "{out}");
}

#[test]
fn const_is_usable_in_an_expression() {
    let c = Case::new("constexpr");
    c.write(
        "main.wlel",
        r#"use std;

const LIMIT: int = 10;

fn main() -> int {
    n := 0;
    while n < LIMIT {
        n += 1;
    }
    std::println_int(n);
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("10"), "{out}");
}

#[test]
fn const_rejects_a_mismatched_type() {
    let c = Case::new("constbad");
    c.write("main.wlel", "const A: int = \"s\";\nfn main() -> int { return 0; }\n");
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("declared int, but the value is string"), "{out}");
}

#[test]
fn const_rejects_a_cycle() {
    let c = Case::new("constcycle");
    c.write(
        "main.wlel",
        "const A: int = B;\nconst B: int = A;\nfn main() -> int { return 0; }\n",
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("must not refer to itself"), "{out}");
}

#[test]
fn const_rejects_a_runtime_value() {
    let c = Case::new("constcall");
    c.write(
        "main.wlel",
        "fn f() -> int { return 1; }\nconst A: int = f();\nfn main() -> int { return 0; }\n",
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("must be a literal expression"), "{out}");
}

#[test]
fn const_crosses_modules() {
    let c = Case::new("constmodule");
    c.write("lib/limits.wlel", "module limits;\n\nconst MAX: int = 100;\nconst NAME: string = \"limits\";\n");
    c.write(
        "main.wlel",
        r#"use std;
use "lib/limits.wlel";

fn main() -> int {
    std::println_int(limits::MAX);
    std::println_str(limits::NAME);
    std::println_int(MAX);
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("100"), "{out}");
    assert!(out.contains("limits"), "{out}");
    assert!(!err.contains("never used"), "the import is used: {err}");
}

// ---- function pointers ----------------------------------------------------

#[test]
fn a_function_name_is_a_value_and_can_be_called() {
    let c = Case::new("fnptr");
    c.write(
        "main.wlel",
        r#"use std;

fn inc(x: int) -> int { return x + 1; }

fn apply(f: fn(int) -> int, x: int) -> int {
    return f(x);
}

fn main() -> int {
    f := inc;
    std::println_int(f(41));
    std::println_int(apply(inc, 10));
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, ["42", "11"], "{out}");
}

#[test]
fn a_function_can_return_a_function_pointer() {
    let c = Case::new("fnret");
    c.write(
        "main.wlel",
        r#"use std;

fn inc(x: int) -> int { return x + 1; }
fn dbl(x: int) -> int { return x * 2; }

fn pick(which: int) -> fn(int) -> int {
    if which == 0 { return inc; }
    return dbl;
}

fn main() -> int {
    g := pick(1);
    std::println_int(g(21));
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("42"), "{out}");
}

#[test]
fn a_function_pointer_can_live_in_a_struct_and_a_vec() {
    let c = Case::new("fnfield");
    c.write(
        "main.wlel",
        r#"use std;

struct Callback {
    f: fn(int) -> int,
}

fn inc(x: int) -> int { return x + 1; }
fn dbl(x: int) -> int { return x * 2; }

fn main() -> int {
    fs := vec_new[Callback]();
    fs.push(Callback { f: inc });
    fs.push(Callback { f: dbl });
    std::println_int((fs.get(0).f)(5));
    std::println_int((fs.get(1).f)(5));
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, ["6", "10"], "{out}");
}

#[test]
fn calling_a_non_function_pointer_is_an_error() {
    let c = Case::new("fnbad");
    c.write(
        "main.wlel",
        "fn main() -> int {\n    n := 1;\n    return n(2);\n}\n",
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("cannot call a value of type int"), "{out}");
}

#[test]
fn a_wrong_arity_through_a_pointer_is_an_error() {
    let c = Case::new("fnarity");
    c.write(
        "main.wlel",
        "fn inc(x: int) -> int { return x + 1; }\nfn main() -> int {\n    f := inc;\n    return f();\n}\n",
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("takes 1 argument(s), got 0"), "{out}");
}

#[test]
fn a_function_pointer_generic_argument_needs_a_wrapper() {
    let c = Case::new("fngeneric");
    c.write(
        "main.wlel",
        "use std;\nfn inc(x: int) -> int { return x + 1; }\nfn main() -> int {\n    fs := vec_new[fn(int) -> int]();\n    return 0;\n}\n",
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("cannot be a type argument of 'Vec'"), "{out}");
    assert!(out.contains("wrap it"), "{out}");
}

// ---- string builder -------------------------------------------------------

#[test]
fn the_string_builder_is_linear_and_correct() {
    let c = Case::new("strbuf");
    c.write(
        "main.wlel",
        r#"use std;

fn main() -> int {
    b := std::str_buf_new();
    i := 0;
    while i < 20000 {
        std::str_buf_push(b, "x");
        i += 1;
    }
    std::str_buf_push(b, " end ");
    std::str_buf_push_int(b, 42);
    std::str_buf_push_float(b, 1.5);
    std::str_buf_push_char(b, 33);
    s := std::str_buf_finish(b);
    std::println_int(std::strlen(s));
    std::println_int(std::str_find(s, " end 421.5!"));
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, ["20011", "20000"], "{out}");
}

#[test]
fn the_string_builder_reports_its_length() {
    let c = Case::new("strbuflen");
    c.write(
        "main.wlel",
        r#"use std;
fn main() -> int {
    b := std::str_buf_new();
    std::str_buf_push(b, "abc");
    std::str_buf_push(b, "de");
    std::println_int(std::str_buf_len(b));
    s := std::str_buf_finish(b);
    std::println_str(s);
    return 0;
}
"#,
    );
    let (out, err, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines, ["5", "abcde"], "{out}");
}

#[test]
fn the_builder_is_much_faster_than_repeated_concatenation() {
    // not a benchmark, just a guard against regressing to O(n^2): 20k
    // appends through the builder must stay far under a second of wall time
    let c = Case::new("strbufperf");
    c.write(
        "main.wlel",
        r#"use std;
fn main() -> int {
    b := std::str_buf_new();
    i := 0;
    while i < 20000 {
        std::str_buf_push(b, "x");
        i += 1;
    }
    std::println_int(std::str_buf_len(b));
    return 0;
}
"#,
    );
    let start = std::time::Instant::now();
    let (out, err, code) = c.run("main.wlel");
    let elapsed = start.elapsed();
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("20000"), "{out}");
    assert!(
        elapsed.as_secs() < 20,
        "20k appends took {elapsed:?} — the builder regressed"
    );
}
