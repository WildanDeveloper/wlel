//! QBE backend experiment (Fase 3 Item 8): `wlel build --backend qbe`
//! transpiles a subset of Wlel to QBE IL. Tests here cover the IL shape,
//! the refusal matrix for out-of-subset constructs, and — when the `qbe`
//! binary is available — end-to-end output parity with the C backend.

use std::fs;
use std::process::Command;

use wlel::checker::Checker;
use wlel::lexer::Lexer;
use wlel::parser::Parser;
use wlel::qbe::gen_program_il;

/// single-file front-end: parse, check, emit QBE IL
fn il(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    Checker::check(&mut p).expect("typecheck");
    gen_program_il(&p).expect("qbe il")
}

fn il_err(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    Checker::check(&mut p).expect("typecheck");
    let e = gen_program_il(&p).expect_err("must be refused");
    assert!(e.contains("qbe backend"), "refusal must mention the qbe backend: {e}");
    e
}

/// is `qbe` usable? (probe once; CI runners without qbe skip the e2e tests)
fn has_qbe() -> bool {
    static OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OK.get_or_init(|| Command::new("qbe").arg("-h").output().is_ok())
}

/// build+run a file through the CLI with the given backend, return stdout
fn run_backend(src_path: &str, out: &str, backend: &str, extra: &[&str]) -> String {
    let bin = env!("CARGO_BIN_EXE_wlel");
    let st = Command::new(bin)
        .args(["build", src_path, "-o", out, "--backend", backend])
        .args(extra)
        .output()
        .expect("spawn wlel");
    assert!(st.status.success(), "wlel build {backend} failed: {}", String::from_utf8_lossy(&st.stderr));
    let run = Command::new(out).output().expect("run binary");
    String::from_utf8_lossy(&run.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// IL shape (no qbe binary needed)

#[test]
fn il_fib_shape() {
    let out = il(
        "fn fib(n: int) -> int {
            if n < 2 {
                return n;
            }
            return fib(n - 1) + fib(n - 2);
        }
        fn main() -> int {
            wlel_print_int(fib(10));
            return 0;
        }",
    );
    assert!(out.contains("function l $fib(l %p0) {"), "{out}");
    assert!(out.contains("@start"), "{out}");
    // signed 64-bit comparison
    assert!(out.contains("csltl"), "{out}");
    // direct recursion
    assert!(out.contains("call $fib(l"), "{out}");
    // entry wrapper: exports $main, saves argc/argv via the runtime
    assert!(out.contains("export function w $main(w %_argc, l %_argv)"), "{out}");
    assert!(out.contains("call $wlel_rt_init(w %_argc, l %_argv)"), "{out}");
    assert!(out.contains("%r =l call $wlel_user_main()"), "{out}");
    // user functions are file-local, main is renamed
    assert!(out.contains("$wlel_user_main"), "{out}");
    assert!(!out.contains("\nfunction $main"), "{out}");
}

#[test]
fn il_widths_and_wrap_fixups() {
    let out = il(
        "fn main() -> int {
            a := 200u8;
            b := 100u8;
            s := a + b;
            wlel_print_int(s as int);
            m := 3i16;
            n := m * -2;
            wlel_print_int(n as int);
            return 0;
        }",
    );
    // sub-32-bit arithmetic is re-canonicalized (Wlel's automatic wrap-cast)
    assert!(out.contains("=w add"), "{out}");
    assert!(out.contains("=w extub"), "{out}");
    assert!(out.contains("=w extsh"), "{out}");
    // narrow values live in word temporaries
    assert!(out.contains("%t"), "{out}");
}

#[test]
fn il_unsigned_ops() {
    let out = il(
        "fn main() -> int {
            a := 4000000000u32;
            b := 2000000000u32;
            wlel_print_int((a + b) as int);
            wlel_print_int((a / 3u32) as int);
            if a > b {
                wlel_print_int(1);
            }
            return 0;
        }",
    );
    assert!(out.contains("=w udiv"), "{out}");
    // unsigned 32-bit comparison
    assert!(out.contains("cugtw"), "{out}");
}

#[test]
fn il_float_ops_and_conversions() {
    let out = il(
        "fn main() -> int {
            x := 2.5;
            y := x * 2.0;
            wlel_print_float(y);
            f := 1.5f32;
            d := f as f64;
            wlel_print_float(d);
            i := 7 / 2;
            wlel_print_float(i as f64 / 2.0);
            wlel_print_int(3.9 as int);
            return 0;
        }",
    );
    assert!(out.contains("=d mul"), "{out}");
    assert!(out.contains("=d exts"), "{out}");
    assert!(out.contains("=d sltof"), "{out}");
    assert!(out.contains("dtosi"), "{out}");
}

#[test]
fn il_arrays_decay_and_slots() {
    let out = il(
        "fn sum(p: *int, n: int) -> int {
            t := 0;
            i := 0;
            while i < n {
                t += p[i];
                i += 1;
            }
            return t;
        }
        fn main() -> int {
            arr := [10, 20, 30];
            wlel_print_int(sum(arr, 3));
            arr[1] = 5;
            wlel_print_int(arr[1]);
            return 0;
        }",
    );
    // arrays are stack slots; passing one decays to the slot address
    assert!(out.contains("=l alloc8 24"), "{out}");
    assert!(out.contains("call $sum(l"), "{out}");
    // element store + load
    assert!(out.contains("storel"), "{out}");
    assert!(out.contains("loadl"), "{out}");
    // every alloc lands in the entry block (QBE requirement)
    let start = out.find("@start").unwrap();
    let first_label_after = out[start + 6..].find('\n').unwrap() + start + 6;
    let next_fn = out[first_label_after..].find("\nfunction").map(|i| i + first_label_after).unwrap_or(out.len());
    let body_until_next_fn = &out[first_label_after..next_fn];
    assert!(!body_until_next_fn.contains("alloc8"), "alloc must be hoisted to @start: {out}");
}

#[test]
fn il_string_literals_and_streq() {
    let out = il(
        r#"fn main() -> int {
            s := "hello";
            if s == "hello" {
                wlel_print_int(1);
            }
            wlel_print_str(s);
            return 0;
        }"#,
    );
    assert!(out.contains(r#"data $wlel_str0 = { b "hello", b 0 }"#), "{out}");
    // string equality is rewritten by the checker to the runtime helper
    assert!(out.contains("call $wlel_streq(l"), "{out}");
}

#[test]
fn il_short_circuit_uses_branches() {
    let out = il(
        "fn main() -> int {
            x := 0;
            safe := x != 0 && 100 / x > 5;
            if !safe {
                wlel_print_int(1);
            }
            return 0;
        }",
    );
    // the right operand is guarded by a conditional jump
    assert!(out.contains("jnz"), "{out}");
    assert!(out.contains("=w copy 0"), "{out}");
}

#[test]
fn il_address_taken_vars_get_slots() {
    let out = il(
        "fn bump(p: *int) {
            *p = *p + 1;
            return;
        }
        fn main() -> int {
            x := 1;
            bump(&x);
            wlel_print_int(x);
            return 0;
        }",
    );
    assert!(out.contains("=l alloc8 8"), "{out}");
    assert!(out.contains("storel"), "{out}");
}

#[test]
fn il_extern_fns_call_directly() {
    let out = il(
        "extern fn sqrt(x: f64) -> f64;
        extern fn atoi(s: string) -> i32;
        fn main() -> int {
            wlel_print_float(sqrt(2.0));
            wlel_print_int(atoi(\"42\") as int);
            return 0;
        }",
    );
    assert!(out.contains("=d call $sqrt(d"), "{out}");
    // i32 return: word result, sign-extended at the call site
    assert!(out.contains("=w call $atoi(l"), "{out}");
    assert!(out.contains("=l extsw"), "{out}");
    // no definition is emitted for extern declarations
    assert!(!out.contains("$sqrt(") || !out.contains("function d $sqrt"), "{out}");
}

#[test]
fn il_void_main_wraps_to_zero() {
    let out = il(
        "fn main() {
            wlel_print_int(7);
        }",
    );
    assert!(out.contains("function $wlel_user_main()"), "{out}");
    assert!(out.contains("\tcall $wlel_user_main()\n\tret 0"), "{out}");
}

#[test]
fn il_sys_builtins() {
    let out = il(
        "fn main() -> int {
            wlel_print_int(sys::argc());
            wlel_print_str(sys::arg(0));
            ok := sys::mono_ms() >= 0;
            if ok {
                wlel_print_int(1);
            }
            return 0;
        }",
    );
    assert!(out.contains("=l call $wlel_rt_argc()"), "{out}");
    assert!(out.contains("=l call $wlel_rt_arg(l"), "{out}");
    assert!(out.contains("=l call $wlel_rt_mono_ms()"), "{out}");
}

// ---------------------------------------------------------------------------
// refusal matrix: everything outside the subset fails with file:line:col

#[test]
fn refuses_use_std() {
    let src = "use std;\nfn main() -> int { std::println_int(1); return 0; }";
    let toks = Lexer::new(src).tokenize().unwrap();
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty());
    Checker::check(&mut p).unwrap();
    let e = gen_program_il(&p).unwrap_err();
    assert!(e.contains("use std"), "{e}");
    assert!(e.contains("1:1"), "{e}");
}

#[test]
fn refuses_structs_enums_impls() {
    let e = il_err("struct Pt { x: float }\nfn main() -> int { return 0; }");
    assert!(e.contains("structs are outside"), "{e}");
    let e = il_err("enum E { A, B }\nfn main() -> int { return 0; }");
    assert!(e.contains("enums are outside"), "{e}");
    let e = il_err("struct Pt { x: float }\nimpl Pt { fn get(self) -> float { return self.x; } }\nfn main() -> int { return 0; }");
    assert!(e.contains("structs are outside"), "{e}");
}

#[test]
fn refuses_defer_arena_new() {
    let e = il_err("fn main() -> int {\n    defer wlel_print_int(1);\n    return 0;\n}");
    assert!(e.contains("defer"), "{e}");
    assert!(e.contains("2:"), "{e}");
    let e = il_err("fn main() -> int {\n    arena(1024) {\n        wlel_print_int(1);\n    }\n    return 0;\n}");
    assert!(e.contains("arena"), "{e}");
    let e = il_err("fn main() -> int {\n    p := new(int);\n    return 0;\n}");
    assert!(e.contains("new()"), "{e}");
}

#[test]
fn monomorphized_generics_are_concrete() {
    // the checker removes unused generic templates and appends used ones as
    // concrete mangled functions — the qbe backend emits those directly
    let out = il(
        "fn id[T](x: T) -> T {\n    return x;\n}\nfn main() -> int {\n    wlel_print_int(id[int](5));\n    return 0;\n}",
    );
    assert!(out.contains("function l $id__int(l %p0)"), "{out}");
    assert!(out.contains("call $id__int(l"), "{out}");
}

#[test]
fn refuses_test_blocks() {
    let e = il_err("fn main() -> int { return 0; }\ntest \"t\" { assert_eq(1, 1); }");
    assert!(e.contains("qbe backend"), "{e}");
    assert!(e.contains("test blocks"), "{e}");
}

#[test]
fn refuses_no_main() {
    let toks = Lexer::new("fn helper() -> int { return 1; }").tokenize().unwrap();
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty());
    Checker::check(&mut p).unwrap();
    let e = gen_program_il(&p).unwrap_err();
    assert!(e.contains("no 'main' function"), "{e}");
}

// ---------------------------------------------------------------------------
// end-to-end parity with the C backend (needs qbe in PATH)

const PARITY_PROGRAMS: &[(&str, &str)] = &[
    (
        "fib",
        r#"
fn fib(n: int) -> int {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
fn main() -> int {
    wlel_print_int(fib(24));
    return 0;
}
"#,
    ),
    (
        "widths_wrap",
        r#"
fn classify(x: i8) -> u8 {
    if x < 0 {
        return 0u8;
    } else if x < 10 {
        return 1u8;
    } else {
        return 2u8;
    }
}
fn main() -> int {
    wlel_print_int((250u8 + 10u8) as int);
    wlel_print_int((255u8 * 2u8) as int);
    wlel_print_int((65535u16 + 1u16) as int);
    wlel_print_int((2147483647i32 + 1i32) as int);
    wlel_print_int((18446744073709551615u64) as int);
    wlel_print_int((-9223372036854775807i64 - 1) as int);
    wlel_print_int(classify(-5) as int);
    wlel_print_int(classify(5) as int);
    wlel_print_int(classify(100) as int);
    wlel_print_int(-16 >> 2);
    wlel_print_int(3 / 2);
    wlel_print_int(-3 / 2);
    wlel_print_int(3 % 2);
    wlel_print_int(-3 % 2);
    wlel_print_int((7u32 % 3u32) as int);
    wlel_print_int((1i16 << 14) as int);
    wlel_print_int((0xF0u8 | 0x0Fu8) as int);
    wlel_print_int((0xFFu8 ^ 0x0Fu8) as int);
    return 0;
}
"#,
    ),
    (
        "floats",
        r#"
fn main() -> int {
    x := 1.5;
    y := x * 3.0;
    wlel_print_float(y);
    wlel_print_float(7.0 / 2.0);
    wlel_print_float(-2.5e10);
    wlel_print_float(x as f32 as f64);
    wlel_print_int(3.9 as int);
    wlel_print_int(-3.9 as int);
    wlel_print_int((2.5 + 0.5) as int);
    f := 2.5f32;
    if f == 2.5f32 {
        wlel_print_int(9);
    }
    if x < 2.0 {
        wlel_print_int(10);
    }
    wlel_print_float(2.0 * -3.5);
    return 0;
}
"#,
    ),
    (
        "control_flow",
        r#"
fn main() -> int {
    acc := 0;
    for i in 0..10 {
        if i == 3 {
            continue;
        }
        if i == 8 {
            break;
        }
        for j in 0..3 {
            acc += 1;
        }
        acc += i;
    }
    wlel_print_int(acc);
    n := 5;
    while n > 0 {
        n -= 2;
    }
    wlel_print_int(n);
    if acc > 100 {
        wlel_print_int(1);
    } else if acc > 50 {
        wlel_print_int(2);
    } else {
        wlel_print_int(3);
    }
    return 0;
}
"#,
    ),
    (
        "pointers_arrays",
        r#"
fn sum(p: *int, n: int) -> int {
    t := 0;
    i := 0;
    while i < n {
        t += p[i];
        i += 1;
    }
    return t;
}
fn reverse(a: *int, n: int) {
    i := 0;
    j := n - 1;
    while i < j {
        tmp := a[i];
        a[i] = a[j];
        a[j] = tmp;
        i += 1;
        j -= 1;
    }
    return;
}
fn bump(p: *int) {
    *p = *p * 2;
    return;
}
fn main() -> int {
    arr := [10, 20, 30, 40, 50];
    wlel_print_int(sum(arr, 5));
    reverse(arr, 5);
    wlel_print_int(arr[0] + arr[4]);
    x := 21;
    bump(&x);
    wlel_print_int(x);
    arr[2] += 7;
    wlel_print_int(arr[2]);
    m := [1, 2, 3, 4];
    i := 0;
    while i < 4 {
        m[i] = m[i] * m[i];
        i += 1;
    }
    wlel_print_int(m[0] + m[1] + m[2] + m[3]);
    wlel_print_int(wlel_sizeof([i16; 7]));
    wlel_print_int(wlel_sizeof(*f64));
    return 0;
}
"#,
    ),
    (
        "strings",
        r#"
fn main() -> int {
    s := "hello";
    wlel_print_str(s);
    if s == "hello" {
        wlel_print_int(1);
    }
    if s != "world" {
        wlel_print_int(2);
    }
    if s == "hellx" {
        wlel_print_int(99);
    }
    wlel_print_int(s[1] as int);
    t := "with\ttab and \\ backslash";
    wlel_print_str(t);
    return 0;
}
"#,
    ),
    (
        "void_main_and_exit",
        r#"
fn main() {
    wlel_print_int(41);
    wlel_print_int(42);
}
"#,
    ),
];

#[test]
fn parity_with_c_backend() {
    if !has_qbe() {
        eprintln!("skipping: qbe not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("wlel_qbe_parity_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for (name, src) in PARITY_PROGRAMS {
        let path = dir.join(format!("{name}.wl"));
        fs::write(&path, src).unwrap();
        let c_out = run_backend(
            path.to_str().unwrap(),
            dir.join(format!("{name}_c")).to_str().unwrap(),
            "c",
            &[],
        );
        let q_out = run_backend(
            path.to_str().unwrap(),
            dir.join(format!("{name}_q")).to_str().unwrap(),
            "qbe",
            &[],
        );
        assert_eq!(c_out, q_out, "backend output mismatch for {name}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn e2e_extern_libm_parity() {
    if !has_qbe() {
        eprintln!("skipping: qbe not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("wlel_qbe_ffi_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ffi.wl");
    fs::write(
        &path,
        "extern fn sqrt(x: f64) -> f64;\nextern fn atoi(s: string) -> i32;\nfn main() -> int {\n    wlel_print_float(sqrt(2.0));\n    wlel_print_int(atoi(\"123\") as int);\n    return 0;\n}\n",
    )
    .unwrap();
    let c_out = run_backend(
        path.to_str().unwrap(),
        dir.join("ffi_c").to_str().unwrap(),
        "c",
        &["-l", "m"],
    );
    let q_out = run_backend(
        path.to_str().unwrap(),
        dir.join("ffi_q").to_str().unwrap(),
        "qbe",
        &["-l", "m"],
    );
    assert_eq!(c_out, q_out);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn e2e_cli_args() {
    if !has_qbe() {
        eprintln!("skipping: qbe not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("wlel_qbe_args_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("args.wl");
    fs::write(
        &path,
        "fn main() -> int {\n    n := sys::argc();\n    wlel_print_int(n);\n    for i in 0..n {\n        wlel_print_str(sys::arg(i));\n    }\n    return 0;\n}\n",
    )
    .unwrap();
    let out = dir.join("args_q");
    let bin = env!("CARGO_BIN_EXE_wlel");
    let st = Command::new(bin)
        .args(["build", path.to_str().unwrap(), "-o", out.to_str().unwrap(), "--backend", "qbe"])
        .output()
        .unwrap();
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let run = Command::new(&out)
        .args(["x", "y"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&run.stdout);
    assert_eq!(text, format!("3\n{}xy", out.to_str().unwrap()));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn e2e_cli_refusal_exit_code() {
    let dir = std::env::temp_dir().join(format!("wlel_qbe_refuse_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("std.wl");
    fs::write(&path, "use std;\nfn main() -> int { std::println_int(1); return 0; }\n").unwrap();
    let bin = env!("CARGO_BIN_EXE_wlel");
    let st = Command::new(bin)
        .args(["build", path.to_str().unwrap(), "-o", dir.join("x").to_str().unwrap(), "--backend", "qbe"])
        .output()
        .unwrap();
    assert!(!st.status.success());
    let err = String::from_utf8_lossy(&st.stderr);
    assert!(err.contains("qbe backend"), "{err}");
    assert!(err.contains("use std"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn e2e_emit_ssa_flag() {
    if !has_qbe() {
        eprintln!("skipping: qbe not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("wlel_qbe_emit_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("m.wl");
    fs::write(&path, "fn main() -> int { return 0; }\n").unwrap();
    let bin = env!("CARGO_BIN_EXE_wlel");
    let st = Command::new(bin)
        .args(["build", path.to_str().unwrap(), "-o", dir.join("m").to_str().unwrap(), "--backend", "qbe", "--emit-c"])
        .output()
        .unwrap();
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let ssa = dir.join("m.ssa");
    let text = fs::read_to_string(&ssa).unwrap();
    assert!(text.contains("export function w $main"), "{text}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn e2e_build_cache_reuse() {
    if !has_qbe() {
        eprintln!("skipping: qbe not installed");
        return;
    }
    let dir = std::env::temp_dir().join(format!("wlel_qbe_cache_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("c.wl");
    fs::write(&path, "fn main() -> int { return 0; }\n").unwrap();
    let bin = env!("CARGO_BIN_EXE_wlel");
    let out = dir.join("c_bin");
    for _ in 0..2 {
        let st = Command::new(bin)
            .args(["build", path.to_str().unwrap(), "-o", out.to_str().unwrap(), "--backend", "qbe"])
            .output()
            .unwrap();
        assert!(st.status.success());
    }
    // the cached binary still runs correctly
    let run = Command::new(&out).output().unwrap();
    assert!(run.status.success());
    let _ = fs::remove_dir_all(&dir);
}
