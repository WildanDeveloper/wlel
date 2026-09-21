//! Sort & search (Fase 2): `std::sort` / `std::binary_search` with a Wlel
//! comparator `(T, T) -> int` over the three collection forms (fixed array,
//! pointer + length, Vec-like), the `str_cmp` stdlib comparator, and the
//! specialized C helpers codegen emits per (element, comparator) pair.

use std::fs;
use std::process::Command;

use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::parser::Parser;
use wlel::stdsrc::{parse_std, STD_FILE};

/// full pipeline including the `use std` splice, exactly like the CLI
fn front(src: &str) -> Result<String, String> {
    let toks = Lexer::new(src).tokenize().map_err(|e| e.to_string())?;
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    let mut std_prog = parse_std();
    for f in std_prog.funcs.iter_mut() {
        f.file = STD_FILE.into();
    }
    for s in std_prog.structs.iter_mut() {
        s.file = STD_FILE.into();
    }
    p.structs.splice(0..0, std_prog.structs);
    p.funcs.splice(0..0, std_prog.funcs);
    for f in p.funcs.iter_mut() {
        if f.file.is_empty() {
            f.file = "sort.wl".into();
        }
    }
    Checker::check(&mut p).map_err(|e| e.msg)?;
    Ok(gen_program(&p))
}

fn check_err_msg(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    let mut std_prog = parse_std();
    for f in std_prog.funcs.iter_mut() {
        f.file = STD_FILE.into();
    }
    for s in std_prog.structs.iter_mut() {
        s.file = STD_FILE.into();
    }
    p.structs.splice(0..0, std_prog.structs);
    p.funcs.splice(0..0, std_prog.funcs);
    for f in p.funcs.iter_mut() {
        if f.file.is_empty() {
            f.file = "sort.wl".into();
        }
    }
    Checker::check(&mut p).expect_err("must be rejected").msg
}

/// run a source file through `wlel run`
fn run_cli(name: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_sort_{name}_{}", std::process::id()));
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

/// build a source file in release mode and run the binary
fn build_and_run(name: &str, src: &str, extra: &[&str]) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_sortb_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join(format!("{name}.wl"));
    fs::write(&path, src).expect("write source");
    let bin = dir.join(name);
    let mut args = vec![
        "build".to_string(),
        path.to_str().unwrap().to_string(),
        "-o".to_string(),
        bin.to_str().unwrap().to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(&args)
        .output()
        .expect("spawn wlel");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&bin).output().expect("run binary");
    let out = (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
    );
    let _ = fs::remove_dir_all(&dir);
    out
}

const ASC: &str = "fn asc(a: int, b: int) -> int { if a < b { return -1; } if a > b { return 1; } return 0; }";

// ---------------------------------------------------------------------------
// codegen: specialized helpers, direct comparator calls, call sites

#[test]
fn helpers_emitted_and_specialized_per_pair() {
    let c = front(
        "use std;
         fn asc(a: int, b: int) -> int { if a < b { return -1; } if a > b { return 1; } return 0; }
         fn asc_f(a: float, b: float) -> int { if a < b { return -1; } if a > b { return 1; } return 0; }
         fn main() -> int {
             a := [5, 4, 3, 2, 1];
             std::sort(a, asc);
             f := [2.5, 1.0];
             std::sort(f, asc_f);
             i := std::binary_search(a, 3, asc);
             s := vec_new[string]();
             vec_push(&s, \"b\");
             vec_push(&s, \"a\");
             std::sort(s, str_cmp);
             return i + s.len;
         }",
    )
    .expect("typecheck");
    // one quicksort per (element, comparator) pair: int, float, string
    assert!(
        c.contains("static void _wlel_sort_0(long long* a, long long lo, long long hi) {"),
        "{c}"
    );
    assert!(
        c.contains("static void _wlel_sort_1(double* a, long long lo, long long hi) {"),
        "{c}"
    );
    assert!(
        c.contains("static void _wlel_sort_2(const char** a, long long lo, long long hi) {"),
        "{c}"
    );
    assert!(
        c.contains("static long long _wlel_bsearch_0(long long* a, long long n, long long key) {"),
        "{c}"
    );
    // comparators are called directly (inlinable), not through void* qsort
    assert!(c.contains("asc(a[i], pivot)"), "{c}");
    assert!(c.contains("asc(a[mid], key)"), "{c}");
    assert!(c.contains("str_cmp(a[i], pivot)"), "{c}");
    assert!(!c.contains("qsort("), "no C qsort dependency: {c}");
    // call sites carry the collection shape
    assert!(c.contains("_wlel_sort_0(a, 0, (5) - 1);"), "{c}");
    assert!(c.contains("_wlel_sort_1(f, 0, (2) - 1);"), "{c}");
    assert!(c.contains("_wlel_bsearch_0(a, 5, 3);"), "{c}");
    assert!(c.contains("_wlel_sort_2(s.data, 0, (s.len) - 1);"), "{c}");
    // str_cmp ships in the stdlib
    assert!(c.contains("static long long str_cmp(const char* a, const char* b);"), "{c}");
}

#[test]
fn std_sort_requires_use_std() {
    let msg = check_err_msg(
        "fn asc(a: int, b: int) -> int { return 0; }
         fn main() -> int { a := [3, 1]; std::sort(a, asc); return 0; }",
    );
    assert!(msg.contains("'std::sort' requires `use std;`"), "{msg}");
    let msg = check_err_msg(
        "fn asc(a: int, b: int) -> int { return 0; }
         fn main() -> int { a := [3, 1]; std::binary_search(a, 1, asc); return 0; }",
    );
    assert!(msg.contains("'std::binary_search' requires `use std;`"), "{msg}");
}

#[test]
fn comparator_errors_are_specific() {
    let cases: &[(&str, &str)] = &[
        ("std::sort(a, 5);", "comparator must be a function name"),
        ("std::sort(a, nope);", "unknown comparator function 'nope'"),
        (
            "std::sort(a, bad_ret);",
            "comparator 'bad_ret' must return int, got bool",
        ),
        (
            "std::sort(a, w32);",
            "comparator 'w32' must take two 'int' arguments, got (i32, i32)",
        ),
        (
            "std::sort(a, one);",
            "comparator 'one' must take two 'int' arguments, got 1 parameter(s)",
        ),
        (
            "std::sort(a, gt);",
            "comparator 'gt' is generic — instantiate it or wrap it in a concrete function",
        ),
    ];
    for (call, want) in cases {
        let src = format!(
            "use std;\n {ASC}\n fn bad_ret(a: int, b: int) -> bool {{ return a < b; }}\n \
             fn w32(a: i32, b: i32) -> int {{ return 0; }}\n \
             fn one(a: int) -> int {{ return 0; }}\n \
             fn gt[T](a: T, b: T) -> int {{ return 0; }}\n \
             fn main() -> int {{ a := [3, 1];\n {call}\n return 0; }}"
        );
        let msg = check_err_msg(&src);
        assert!(msg.contains(want), "want {want:?} got: {msg}");
    }
}

#[test]
fn collection_form_errors_are_specific() {
    let cases: &[(&str, &str)] = &[
        (
            "std::sort(a, 5, asc);",
            "std::sort: an array carries its length — pass (arr, cmp)",
        ),
        (
            "std::binary_search(a, 2, 3, asc);",
            "std::binary_search: an array carries its length — pass (arr, needle, cmp)",
        ),
        (
            "std::sort(p, asc);",
            "std::sort: a pointer needs an explicit length — pass (pointer, n, comparator)",
        ),
        (
            "std::sort(p, \"x\", asc);",
            "std::sort: n must be int, got string",
        ),
        (
            "std::sort(5, asc);",
            "std::sort: cannot sort a value of type 'int' — pass an array, a (pointer, length) pair or a Vec-like struct",
        ),
        (
            "std::sort(pt, asc);",
            "std::sort: 'Pt' has no data/len element buffer — pass an array or a (pointer, length) pair",
        ),
        (
            "std::sort(g, asc);",
            "std::sort: element type '[int; 2]' is not sortable — arrays of arrays are not supported",
        ),
        (
            "std::binary_search(a, \"x\", asc);",
            "std::binary_search: needle must be int, got string",
        ),
    ];
    for (call, want) in cases {
        let src = format!(
            "use std;\n {ASC}\n struct Pt {{ x: int }}\n \
             fn main() -> int {{ a := [3, 1]; p := new(int, 2); pt := Pt {{ x: 1 }}; g := [[1, 2], [3, 4]];\n {call}\n return 0; }}"
        );
        let msg = check_err_msg(&src);
        assert!(msg.contains(want), "want {want:?} got: {msg}");
    }
}

#[test]
fn vec_sugar_and_pointer_forms_typecheck() {
    front(
        "use std;
         fn asc(a: int, b: int) -> int { return 0; }
         fn main() -> int {
             v := vec_new[int]();
             vec_push(&v, 2);
             std::sort(v, asc);
             std::binary_search(v, 2, asc);
             p := new(int, 2);
             std::sort(p, 2, asc);
             std::binary_search(p, 2, 1, asc);
             return 0;
         }",
    )
    .expect("all three forms accepted");
}

// ---------------------------------------------------------------------------
// end-to-end behavior via the CLI

#[test]
fn sort_and_search_end_to_end() {
    let src = r#"use std;
        fn asc(a: int, b: int) -> int { if a < b { return -1; } if a > b { return 1; } return 0; }
        fn main() -> int {
            a := [5, 3, 9, 1];
            std::sort(a, asc);
            for x in a { std::println_int(x); }
            p := new(int, 3);
            p[0] = 30; p[1] = 10; p[2] = 20;
            std::sort(p, 3, asc);
            i := std::binary_search(p, 3, 20, asc);
            std::println_str(std::format("idx={}", i));
            return 0;
        }"#;
    let (ok, out, err) = run_cli("e2e", src);
    assert!(ok, "{err}");
    assert_eq!(out, "1\n3\n5\n9\nidx=1\n", "{out}");
}

#[test]
fn string_sort_uses_str_cmp_end_to_end() {
    let src = r#"use std;
        fn main() -> int {
            w := vec_new[string]();
            vec_push(&w, "pear");
            vec_push(&w, "apple");
            vec_push(&w, "fig");
            std::sort(w, str_cmp);
            for s in w { std::println_str(s); }
            i := std::binary_search(w, "fig", str_cmp);
            std::println_str(std::format("fig@{}", i));
            return 0;
        }"#;
    let (ok, out, err) = run_cli("streq", src);
    assert!(ok, "{err}");
    assert_eq!(out, "apple\nfig\npear\nfig@1\n", "{out}");
}

// ---------------------------------------------------------------------------
// the production criterion: 1,000,000 ints sorted under 100 ms (release)

#[test]
fn one_million_ints_sort_under_100ms_release() {
    let src = r#"use std;
        fn asc(a: int, b: int) -> int { if a < b { return -1; } if a > b { return 1; } return 0; }
        fn main() -> int {
            n := 1_000_000;
            data := new(int, n);
            seed := 0x2545F4914F6CDD1Du64;
            for i in 0..n {
                seed = seed * 6364136223846793005 + 1442695040888963407;
                data[i] = (seed >> 33) as int;
            }
            t0 := sys::mono_ms();
            std::sort(data, n, asc);
            ms := sys::mono_ms() - t0;
            ordered := true;
            for i in 1..n {
                if data[i - 1] > data[i] { ordered = false; }
            }
            std::println_str(std::format("ms={} ordered={}", ms, ordered));
            return 0;
        }"#;
    let (ok, out, err) = build_and_run("bench1m", src, &["-O2"]);
    assert!(ok, "{err}");
    let line = out.lines().last().expect("result line");
    let ms: i64 = line["ms=".len()..].split(' ').next().expect("ms").parse().expect("int");
    assert!(line.contains("ordered=true"), "{out}");
    // criterion: < 100 ms (measured ~77 ms here; CI machines get headroom)
    assert!(ms < 200, "1M int sort took {} ms: {line}", ms);
}

// ---------------------------------------------------------------------------
// memory safety & release purity

#[cfg(unix)]
#[test]
fn sort_program_is_asan_clean() {
    let dir = std::env::temp_dir().join(format!("wlel_sortasan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("asan.wl");
    fs::write(
        &path,
        r#"use std;
           fn asc(a: int, b: int) -> int { if a < b { return -1; } if a > b { return 1; } return 0; }
           fn main() -> int {
               arena(1 << 16) {
                   a := [9, 1, 5, 3, 7];
                   std::sort(a, asc);
                   assert_eq(a[0], 1);
                   w := vec_new[string]();
                   vec_push(&w, "z");
                   vec_push(&w, "a");
                   vec_push(&w, "m");
                   std::sort(w, str_cmp);
                   assert_eq(w.data[0], "a");
                   assert_eq(std::binary_search(a, 5, asc), 2);
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

#[test]
fn release_build_has_no_check_overhead_for_sort() {
    let c = front(
        "use std;
         fn asc(a: int, b: int) -> int { return 0; }
         fn main() -> int {
             a := [3, 1, 2];
             std::sort(a, asc);
             return std::binary_search(a, 2, asc);
         }",
    )
    .expect("typecheck");
    assert!(c.contains("_wlel_sort_0("), "{c}");
    assert!(!c.contains("_wlel_arr_at"), "release must be check-free: {c}");
    assert!(!c.contains("_wlel_divz"), "{c}");
}

// ---------------------------------------------------------------------------
// dogfood: the example's own test blocks must run green

#[test]
fn example_sort_test_blocks_pass() {
    let test = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", "examples/sort.wl"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawn wlel");
    assert!(test.status.success(), "{}", String::from_utf8_lossy(&test.stderr));
    let out = String::from_utf8_lossy(&test.stdout);
    assert!(out.contains("6 passed, 0 failed"), "{out}");
}
