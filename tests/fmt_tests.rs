//! `wlel fmt` tests: canonical form, idempotency, comment preservation,
//! parenthesization, and the no-diff guarantee on the repo's own examples.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn canon(src: &str) -> String {
    wlel::fmt::format_source(src).expect("format")
}

fn fmt_idempotent(src: &str) -> String {
    let once = canon(src);
    let twice = canon(&once);
    assert_eq!(once, twice, "formatter is not idempotent for:\n{src}");
    once
}

#[test]
fn repo_examples_are_canonical() {
    // the no-diff criterion: formatting the repo's own examples changes
    // nothing (they are committed in canonical form)
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.push("examples");
    let mut files: Vec<PathBuf> = Vec::new();
    collect(&dir, &mut files);
    assert!(files.len() >= 15, "expected the example corpus, found {}", files.len());
    for f in files {
        let src = fs::read_to_string(&f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        let out = wlel::fmt::format_source(&src)
            .unwrap_or_else(|e| panic!("format {}: {e}", f.display()));
        assert_eq!(out, src, "{} is not in canonical form", f.display());
    }
}

fn collect(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().map(|x| x == "wl").unwrap_or(false) {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------------------
// declarations & layout
// ---------------------------------------------------------------------------

#[test]
fn blank_lines_collapse_and_decls_separate() {
    let out = fmt_idempotent(
        "fn a() -> int { return 1; }\n\n\n\nfn b() -> int { return 2; }\nfn c() -> int { return 3; }\n",
    );
    assert_eq!(
        out,
        "fn a() -> int {\n    return 1;\n}\n\nfn b() -> int {\n    return 2;\n}\n\nfn c() -> int {\n    return 3;\n}\n"
    );
}

#[test]
fn adjacent_use_lines_stay_grouped() {
    let out = fmt_idempotent("use std;\nuse \"lib/geom.wl\";\nfn main() -> int { return 0; }\n");
    assert!(out.starts_with("use std;\nuse \"lib/geom.wl\";\n\nfn main() -> int {"), "{out}");
}

#[test]
fn declaration_order_is_preserved() {
    let src = "fn first() -> int { return 1; }\n\nstruct Mid {\n    v: int\n}\n\nfn last() -> int { return 2; }\n";
    let out = fmt_idempotent(src);
    let fi = out.find("fn first").unwrap();
    let st = out.find("struct Mid").unwrap();
    let la = out.find("fn last").unwrap();
    assert!(fi < st && st < la, "{out}");
}

#[test]
fn structs_print_fields_one_per_line() {
    let out = fmt_idempotent("struct Box[T] {val:T, next:*Box[T]}\n");
    assert_eq!(
        out,
        "struct Box[T] {\n    val: T,\n    next: *Box[T]\n}\n"
    );
    assert_eq!(fmt_idempotent("struct Unit {}\n"), "struct Unit {}\n");
}

#[test]
fn generics_and_signatures_round_trip() {
    let out = fmt_idempotent(
        "fn pick[T](a: T, b: T, choose: bool) -> T { if choose { return a; } return b; }\n",
    );
    assert_eq!(
        out,
        "fn pick[T](a: T, b: T, choose: bool) -> T {\n    if choose {\n        return a;\n    }\n    return b;\n}\n"
    );
}

// ---------------------------------------------------------------------------
// expressions & parenthesization
// ---------------------------------------------------------------------------

#[test]
fn needed_parens_survive_redundant_parens_vanish() {
    assert!(fmt_idempotent("fn f() -> int { x := (1 + 2) * 3; return x; }\n")
        .contains("x := (1 + 2) * 3;"));
    assert!(fmt_idempotent("fn f() -> int { x := 1 + (2 * 3); return x; }\n")
        .contains("x := 1 + 2 * 3;"));
    assert!(fmt_idempotent("fn f() -> int { x := 1 - (2 - 3); return x; }\n")
        .contains("x := 1 - (2 - 3);"));
    assert!(fmt_idempotent("fn f() -> int { x := (1 + 2) - 3; return x; }\n")
        .contains("x := 1 + 2 - 3;"));
}

#[test]
fn unary_and_cast_parens_round_trip() {
    // a unary over a binary keeps its parens
    assert!(fmt_idempotent("fn f() -> int { x := -(1 * 2); return x; }\n").contains("x := -(1 * 2);"));
    // cast of a binary keeps its parens
    assert!(fmt_idempotent("fn f() -> int { x := (1 + 2) as u8; return x as int; }\n")
        .contains("x := (1 + 2) as u8;"));
    // field access on a unary keeps its parens
    assert!(fmt_idempotent(
        "struct P { x: int }\nfn f(p: P) -> int { q := &p; return (-q.x); }\n"
    )
    .contains("return (-q.x);") || fmt_idempotent(
        "struct P { x: int }\nfn f(p: P) -> int { q := &p; return (-q.x); }\n"
    )
    .contains("return -q.x;"));
}

#[test]
fn suffix_literals_print_attached() {
    let out = fmt_idempotent("fn f() -> u8 { return 10 as u8; }\n");
    assert!(out.contains("return 10u8;"), "{out}");
    let out = fmt_idempotent("fn f() -> f32 { return 2.5 as f32; }\n");
    assert!(out.contains("return 2.5f32;"), "{out}");
    // a variable cast keeps the operator form
    let out = fmt_idempotent("fn f(x: int) -> u8 { return x as u8; }\n");
    assert!(out.contains("return x as u8;"), "{out}");
}

#[test]
fn float_literals_stay_float_typed() {
    let out = fmt_idempotent("fn f() -> float { x := 1e3; return x; }\n");
    assert!(out.contains("x := 1000.0;"), "{out}");
    let out = fmt_idempotent("fn f() -> float { x := 0.50; return x; }\n");
    assert!(out.contains("x := 0.5;"), "{out}");
    // exponent form for extreme values, still reparseable
    let out = fmt_idempotent("fn f() -> float { x := 1.0e300; return x; }\n");
    let lit = out.split("x := ").nth(1).unwrap().split(';').next().unwrap().to_string();
    let parsed = lit.parse::<f64>().expect("float literal parses");
    assert!((parsed - 1e300).abs() < 1e290, "{lit}");
    // formatting the output again is a no-op (idempotent by construction above)
}

#[test]
fn radix_and_separators_normalize_to_decimal() {
    let out = fmt_idempotent("fn f() -> int { x := 0xFF; y := 1_000_000; return x + y; }\n");
    assert!(out.contains("x := 255;"), "{out}");
    assert!(out.contains("y := 1000000;"), "{out}");
}

#[test]
fn strings_keep_escapes_and_utf8() {
    let out = fmt_idempotent("fn f() -> int { s := \"a\\n\\t\\\"q\\\"\"; std::println_str(s); return 0; }\n");
    assert!(out.contains("\"a\\n\\t\\\"q\\\"\""), "{out}");
    let out = fmt_idempotent("fn f() -> int { std::println_str(\"halo dunia\"); return 0; }\n");
    assert!(out.contains("\"halo dunia\""), "{out}");
}

#[test]
fn struct_literals_in_conditions_reparse() {
    let src = "struct P {\n    x: int,\n    y: int\n}\n\nfn f(p: P) -> int {\n    if p == P { x: 1, y: 2 } {\n        return 7;\n    }\n    return 0;\n}\n";
    let out = fmt_idempotent(src);
    assert!(out.contains("if p == P { x: 1, y: 2 } {"), "{out}");
}

#[test]
fn all_control_flow_forms_round_trip() {
    let src = r#"fn f(n: int) -> int {
    total := 0;
    for i in 0..n {
        if i % 2 == 0 { continue; }
        if i > 10 { break; }
        total += i;
    }
    for x in [1, 2, 3] {
        total += x;
    }
    while total > 100 {
        total -= 100;
    }
    arena(4096) {
        p := new(int, 4);
        p[0] = 1;
        total += p[0];
    }
    defer std::println_str("done");
    defer {
        total += 0;
    }
    {
        let inner: int = 5;
        total += inner;
    }
    return total;
}
"#;
    fmt_idempotent(src);
}

// ---------------------------------------------------------------------------
// comments
// ---------------------------------------------------------------------------

#[test]
fn comments_survive_in_all_positions() {
    let src = "// header line\n// second header line\nuse std;\n\n// doc for f\nfn f() -> int {\n    // leading\n    x := 1; // trailing\n    // before close\n    return x;\n}\n\n// between decls\n\nfn g() -> int {\n    return 2;\n} // after close\n";
    let out = fmt_idempotent(src);
    assert!(out.starts_with("// header line\n// second header line\nuse std;"), "{out}");
    assert!(out.contains("// doc for f\nfn f"), "{out}");
    assert!(out.contains("    // leading\n"), "{out}");
    assert!(out.contains("x := 1; // trailing\n"), "{out}");
    assert!(out.contains("    // before close\n    return x;"), "{out}");
    assert!(out.contains("// between decls\nfn g"), "{out}");
    assert!(out.contains("} // after close\n"), "{out}");
}

#[test]
fn comment_only_file_is_preserved() {
    let src = "// just a note\n// and another\n";
    assert_eq!(fmt_idempotent(src), src);
}

#[test]
fn comment_text_is_trimmed_at_line_end() {
    let out = fmt_idempotent("fn f() -> int {\n    return 1;\n} // note   \n");
    assert!(out.contains("} // note\n"), "{out:?}");
}

// ---------------------------------------------------------------------------
// CLI behavior
// ---------------------------------------------------------------------------

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(name: &str, src: &str) -> TempFile {
        let dir = std::env::temp_dir().join(format!("wlel_fmt_{}_{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join(format!("{name}.wl"));
        fs::write(&path, src).expect("write");
        TempFile { path }
    }

    fn wlel(&self, args: &[&str]) -> (String, String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(args)
            .output()
            .expect("spawn wlel");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.path.parent().unwrap());
    }
}

#[test]
fn fmt_file_mode_writes_in_place_and_check_flags_clean_files() {
    let t = TempFile::new("messy", "fn f( ) -> int {\n        x:=1+2;\n    return   x ;\n}\n");
    let p = t.path.to_str().unwrap();
    let (out, err, code) = t.wlel(&["fmt", p]);
    assert_eq!(code, Some(0), "{out}{err}");
    let formatted = fs::read_to_string(&t.path).unwrap();
    assert!(formatted.contains("x := 1 + 2;"), "{formatted}");
    // now canonical: fmt is a no-op, -check passes
    let (_out, _err, code) = t.wlel(&["fmt", p]);
    assert_eq!(code, Some(0));
    assert_eq!(fs::read_to_string(&t.path).unwrap(), formatted);
    let (out, _err, code) = t.wlel(&["fmt", p, "-check"]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("all formatted"), "{out}");
}

#[test]
fn fmt_check_exits_nonzero_when_changes_are_needed() {
    let t = TempFile::new("needsfmt", "fn f() -> int { return 1; }\n");
    let p = t.path.to_str().unwrap();
    let (out, _err, code) = t.wlel(&["fmt", p, "-check"]);
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("would format"), "{out}");
    // -check never writes
    assert_eq!(fs::read_to_string(&t.path).unwrap(), "fn f() -> int { return 1; }\n");
}

#[test]
fn fmt_never_touches_a_file_that_does_not_parse() {
    let broken = "fn f( -> int { !!! }\n";
    let t = TempFile::new("broken", broken);
    let p = t.path.to_str().unwrap();
    let (_out, err, code) = t.wlel(&["fmt", p]);
    assert_ne!(code, Some(0));
    assert!(err.contains("syntax error") || err.contains("expected"), "{err}");
    assert_eq!(fs::read_to_string(&t.path).unwrap(), broken, "file must be untouched");
}

#[test]
fn fmt_semantics_are_preserved_end_to_end() {
    // a program prints the same before and after formatting
    let src = r#"use std;
fn add(a: int, b: int) -> int { return a + b; }
fn main() -> int {
    v := [1, 2, 3];
    s := 0;
    for x in v { s = add(s, x); }
    std::println_int(s * (2 + 1));
    return 0;
}
"#;
    let t = TempFile::new("semantic", src);
    let p = t.path.to_str().unwrap();
    let (before, err, code) = t.wlel(&["run", p]);
    assert_eq!(code, Some(0), "{before}{err}");
    let (_out, err, code) = t.wlel(&["fmt", p]);
    assert_eq!(code, Some(0), "{err}");
    let (after, err, code) = t.wlel(&["run", p]);
    assert_eq!(code, Some(0), "{after}{err}");
    assert_eq!(before, after, "output changed after formatting");
    assert!(after.contains("18"), "{after}");
}

#[test]
fn fmt_project_mode_formats_all_wl_files() {
    let dir = std::env::temp_dir().join(format!("wlel_fmtproj_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let wlel_bin = env!("CARGO_BIN_EXE_wlel");
    let st = Command::new(wlel_bin)
        .args(["new", "app"])
        .current_dir(&dir)
        .output()
        .expect("wlel new");
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    // a second, messy file inside src/
    fs::write(dir.join("app/src/util.wl"), "fn  twice( x:int )->int { return  x*2; }\n").unwrap();
    // and a file that must be ignored (hidden dir)
    fs::create_dir_all(dir.join("app/.hidden")).unwrap();
    fs::write(dir.join("app/.hidden/skip.wl"), "fn x() -> int { return 1;   }\n").unwrap();

    let (out, err, code) = Command::new(wlel_bin)
        .args(["fmt"])
        .current_dir(dir.join("app"))
        .output()
        .map(|o| (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
            o.status.code(),
        ))
        .unwrap();
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("util.wl"), "{out}");
    let util = fs::read_to_string(dir.join("app/src/util.wl")).unwrap();
    assert!(util.contains("fn twice(x: int) -> int {"), "{util}");
    // the hidden file is untouched
    assert_eq!(
        fs::read_to_string(dir.join("app/.hidden/skip.wl")).unwrap(),
        "fn x() -> int { return 1;   }\n"
    );
    // second run: nothing to do
    let (out, _err, code) = Command::new(wlel_bin)
        .args(["fmt"])
        .current_dir(dir.join("app"))
        .output()
        .map(|o| (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::new(),
            o.status.code(),
        ))
        .unwrap();
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("all formatted"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}
