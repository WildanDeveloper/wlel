use wlel::checker::{CheckError, CheckWarning};
use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::parser::Parser;

fn parse_ok(src: &str) -> wlel::ast::Program {
    let toks = Lexer::new(src).tokenize().expect("lex");
    let (p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
    p
}

fn check(src: &str) -> Result<(), String> {
    let mut p = parse_ok(src);
    Checker::check(&mut p).map(|_| ()).map_err(|e| e.msg)
}

/// like `check`, but also returns the non-fatal warnings for inspection
fn check_warn(src: &str) -> Result<Vec<CheckWarning>, String> {
    let mut p = parse_ok(src);
    Checker::check(&mut p).map_err(|e| e.msg)
}

fn check_err(src: &str) -> Result<(), CheckError> {
    let mut p = parse_ok(src);
    Checker::check(&mut p).map(|_| ())
}

#[test]
fn valid_program_passes() {
    assert!(check(
        "fn add(a: int, b: int) -> int { return a + b; }
         fn main() -> int {
             x := add(1, 2);
             if x > 0 { return 0; }
             return 1;
         }"
    ).is_ok());
}

#[test]
fn undefined_variable() {
    let e = check("fn main() -> int { return nope; }").unwrap_err();
    assert!(e.contains("undefined variable 'nope'"), "{e}");
}

#[test]
fn undefined_variable_span() {
    let src = "fn main() -> int {\n    x := 1;\n    return nope;\n}";
    let err = check_err(src).unwrap_err();
    assert_eq!(err.span.start.line, 3);
    assert_eq!(err.span.start.col, 12);
    assert!(err.msg.contains("undefined variable 'nope'"), "{}", err.msg);
}

#[test]
fn type_mismatch_span_points_at_let() {
    let src = "fn main() -> int {\n    let s: string = 5;\n    return 0;\n}";
    let err = check_err(src).unwrap_err();
    assert_eq!(err.span.start.line, 2);
}

#[test]
fn type_mismatch_on_let_annotation() {
    let e = check(r#"fn main() -> int { let s: string = 5; return 0; }"#).unwrap_err();
    assert!(e.contains("cannot initialize"), "{e}");
}

#[test]
fn unknown_type() {
    let e = check("fn f(x: banana) -> int { return 0; }").unwrap_err();
    assert!(e.contains("unknown type 'banana'"), "{e}");
}

#[test]
fn mixed_width_program_passes() {
    assert!(check(
        "fn main() -> int {
             let a: u8 = 200;
             let b: i32 = 100_000;
             let c: u64 = 1;
             let d: usize = 2;
             let e: f32 = 1.5f32;
             let f: byte = 7;
             let g: char = 9;
             let h: i8 = -100;
             let i: u16 = 60_000;
             let j: i16 = -30_000;
             let k: u32 = 4_000_000_000;
             let m: f64 = 2.5;
             let n: u64 = k as u64;
             return (a as int)
                 + (b as int)
                 + (c as int)
                 + (d as int)
                 + (e as int)
                 + (f as int)
                 + (g as int)
                 + (h as int)
                 + (i as int)
                 + (j as int)
                 + (k as int)
                 + (m as int)
                 + (n as int);
         }"
    )
    .is_ok());
}

#[test]
fn narrowing_variable_requires_cast() {
    let e = check(
        "fn f(x: int) -> int {
             let y: u8 = x;
             return y as int;
         }",
    )
    .unwrap_err();
    assert!(e.contains("cannot initialize 'y' of type u8 with int"), "{e}");
}

#[test]
fn narrowing_requires_explicit_cast() {
    // u64 -> u32 is a narrowing: cast must be explicit
    let e = check(
        "fn f(x: u64) -> int {
             let y: u32 = x;
             return y as int;
         }",
    )
    .unwrap_err();
    assert!(e.contains("cannot initialize 'y' of type u32 with u64"), "{e}");

    // ...but an explicit cast is fine
    assert!(check(
        "fn f(x: u64) -> int {
             let y: u32 = x as u32;
             return y as int;
         }"
    )
    .is_ok());
}

#[test]
fn literal_range_checked() {
    let e = check("fn main() -> int { let x: u8 = 256; return 0; }").unwrap_err();
    assert!(e.contains("literal 256 does not fit in type u8"), "{e}");

    let e2 = check("fn main() -> int { let x: i8 = -129; return 0; }").unwrap_err();
    assert!(e2.contains("literal -129 does not fit in type i8"), "{e2}");

    let e3 = check("fn main() -> int { let x: u8 = -1; return 0; }").unwrap_err();
    assert!(e3.contains("literal -1 does not fit in type u8"), "{e3}");

    // boundary values are fine
    assert!(check(
        "fn main() -> int {
             let a: u8 = 255;
             let b: i8 = -128;
             let c: u64 = 0;
             return 0;
         }"
    )
    .is_ok());
}

#[test]
fn literal_adapts_to_variable_width() {
    // u8 + untyped literal stays u8
    assert!(check(
        "fn f(x: u8) -> int {
             let y: u8 = x + 1;
             return y as int;
         }"
    )
    .is_ok());

    // but an out-of-range literal on a u8 variable is caught
    let e = check("fn f(x: u8) -> int { return x + 256; }").unwrap_err();
    assert!(e.contains("literal 256 does not fit in type u8"), "{e}");
}

#[test]
fn mixed_width_arithmetic_requires_cast() {
    let e = check(
        "fn f(a: i32, b: u64) -> int {
             return (a + b) as int;
         }",
    )
    .unwrap_err();
    assert!(e.contains("mixed int widths: i32 and u64"), "{e}");

    // comparison across widths too
    let e2 = check("fn f(a: i32, b: u64) -> bool { return a < b; }").unwrap_err();
    assert!(e2.contains("mixed int widths: i32 and u64"), "{e2}");
}

#[test]
fn small_width_arithmetic_wraps() {
    // checker inserts an explicit wrap cast for sub-32-bit results
    let mut p = parse_ok(
        "fn f(x: u8) -> int {
             let y: u8 = x + 1;
             return y as int;
         }",
    );
    Checker::check(&mut p).expect("check");
    let c = gen_program(&p);
    assert!(c.contains("uint8_t y = (uint8_t)((x + 1));"), "{c}");
}

#[test]
fn call_unknown_function() {
    let e = check("fn main() -> int { ghost(); return 0; }").unwrap_err();
    assert!(e.contains("undefined function 'ghost'"), "{e}");
}

#[test]
fn arg_count_and_types() {
    let e = check(
        "fn add(a: int, b: int) -> int { return a + b; }
         fn main() -> int { return add(1); }",
    )
    .unwrap_err();
    assert!(e.contains("takes 2 argument(s)"), "{e}");

    let e2 = check(
        "fn take_str(s: string) -> int { return 0; }
         fn main() -> int { return take_str(3); }",
    )
    .unwrap_err();
    assert!(e2.contains("expected string"), "{e2}");
}

#[test]
fn mod_needs_ints() {
    let e = check("fn main() -> float { return 1.5 % 2.0; }").unwrap_err();
    assert!(e.contains("'Mod'"), "{e}");
}

#[test]
fn comparison_returns_bool() {
    assert!(check(
        "fn main() -> bool { return 1 < 2 && true; }"
    ).is_ok());
    let e = check("fn main() -> int { return (1 < 2) + 1; }").unwrap_err();
    assert!(e.contains("cannot apply"), "{e}");
}

#[test]
fn return_type_mismatch() {
    let e = check("fn f() -> int { return 1.5; }").unwrap_err();
    assert!(e.contains("return type mismatch"), "{e}");
    let e2 = check("fn g() { return 1; }").unwrap_err();
    assert!(e2.contains("void function cannot return"), "{e2}");
    // bare return in void fn is fine
    assert!(check("fn g() { return; }").is_ok());
}

#[test]
fn missing_return_detected() {
    let e = check("fn f(a: int) -> int { if a > 0 { return 1; } }").unwrap_err();
    assert!(e.contains("no return statement"), "{e}");
    // but both branches returning is fine
    assert!(check(
        "fn f(a: int) -> int { if a > 0 { return 1; } else { return 2; } }"
    )
    .is_ok());
}

#[test]
fn shadowing_allowed() {
    assert!(check(
        "fn main() -> int {
            x := 1;
            if true { x := \"now a string\"; }
            return 0;
        }"
    )
    .is_ok());
}

#[test]
fn duplicate_function() {
    let e = check("fn f() -> int { return 0; } fn f() -> int { return 1; }").unwrap_err();
    assert!(e.contains("duplicate function"), "{e}");
}

#[test]
fn forward_reference_ok() {
    assert!(check(
        "fn main() -> int { return later(2); }
         fn later(x: int) -> int { return x * 10; }"
    )
    .is_ok());
}

#[test]
fn structs_and_pointers() {
    // happy path: literal, addr-of, auto-deref field, mutation through pointer
    assert!(check(
        "struct Pt { x: float, y: float }
         fn bump(p: *Pt) -> void { p.x = p.x + 1.0; }
         fn main() -> int {
             let o: Pt = Pt { x: 0.0, y: 2.5 };
             let q: *Pt = &o;
             bump(q);
             if o.x == 1.0 { return 0; }
             return 1;
         }"
    )
    .is_ok());

    // unknown struct
    let e = check("fn main() -> int { let g: Ghost = 1; return 0; }").unwrap_err();
    assert!(e.contains("unknown type 'Ghost'"), "{e}");

    // bad field
    let e = check(
        "struct Pt { x: int }
         fn main() -> int { let o: Pt = Pt { x: 1 }; return o.nope; }"
    ).unwrap_err();
    assert!(e.contains("no field 'nope'"), "{e}");

    // struct literal field mismatch
    let e = check(
        "struct Pt { x: int }
         fn main() -> int { let o: Pt = Pt { x: 1.5 }; return 0; }"
    ).unwrap_err();
    assert!(e.contains("field 'x': expected int"), "{e}");

    // & of non-lvalue rejected
    let e = check(
        "struct Pt { x: int }
         fn main() -> int { let p: *Pt = &Pt { x: 1 }; return 0; }"
    ).unwrap_err();
    assert!(e.contains("'&' needs a variable"), "{e}");

    // deref of non-pointer
    let e = check("fn main() -> int { let x: int = 1; return *x; }").unwrap_err();
    assert!(e.contains("cannot dereference non-pointer"), "{e}");

    // field on non-struct
    let e = check("fn main() -> int { let x: int = 1; return x.nope; }").unwrap_err();
    assert!(e.contains("non-struct"), "{e}");
}

#[test]
fn defer_must_be_void() {
    let e = check("fn f() -> int { defer 5; return 0; }").unwrap_err();
    assert!(e.contains("defer needs a void expression"), "{e}");
    assert!(check(
        "fn log() -> void { return; }
         fn main() -> int { defer log(); return 0; }"
    )
    .is_ok());
}

#[test]
fn arrays_and_casts() {
    // array decay to pointer param
    assert!(check(
        "fn sum(a: *int, n: int) -> int { return a[0]; }
         fn main() -> int { let x: [int; 2] = [1, 2]; return sum(x, 2); }"
    )
    .is_ok());

    // whole-array assignment rejected
    let e = check(
        "fn main() -> int { let a: [int; 2] = [1, 2]; a = [3, 4]; return 0; }"
    ).unwrap_err();
    assert!(e.contains("not assignable"), "{e}");

    // array literal too long
    let e = check(
        "fn main() -> int { let a: [int; 2] = [1, 2, 3]; return 0; }"
    ).unwrap_err();
    assert!(e.contains("holds 2"), "{e}");

    // casts
    assert!(check("fn main() -> int { let f: float = 1.5; return f as int; }").is_ok());
    let e = check("fn main() -> int { return 1 as string; }").unwrap_err();
    assert!(e.contains("invalid cast"), "{e}");

    // sizeof via type
    assert!(check(
        "struct Pt { x: int }
         fn main() -> int { return wlel_sizeof(Pt); }"
    )
    .is_ok());
}

#[test]
fn std_module_gate() {
    // without `use std;` the std functions are unavailable
    let e = check("fn main() -> int { std::println_int(1); return 0; }").unwrap_err();
    assert!(e.contains("requires `use std;`"), "{e}");
    assert!(check(
        "use std;
         fn main() -> int { std::println_int(1); return 0; }"
    )
    .is_ok());
}

#[test]
fn break_continue_outside_loop_rejected() {
    let e1 = check("fn main() -> int { break; return 0; }").unwrap_err();
    assert!(e1.contains("'break' outside of loop"), "{e1}");
    let e2 = check("fn main() -> int { continue; return 0; }").unwrap_err();
    assert!(e2.contains("'continue' outside of loop"), "{e2}");
}

#[test]
fn break_continue_inside_loop_accepted() {
    assert!(check(
        "fn main() -> int {
            i := 0;
            while i < 10 {
                if i == 5 { break; }
                i = i + 1;
                continue;
            }
            return i;
        }"
    ).is_ok());
}

#[test]
fn bitwise_ops_reject_non_ints() {
    let e1 = check("fn main() -> int { return 1.5 & 2; }").unwrap_err();
    assert!(e1.contains("needs int"), "{e1}");
    let e2 = check("fn main() -> int { return ~1.5; }").unwrap_err();
    assert!(e2.contains("'~' needs int"), "{e2}");
}

#[test]
fn compound_assign_rules() {
    assert!(check("fn main() -> int { x := 1; x += 2; return x; }").is_ok());
    let e = check("fn main() -> int { let s: string = \"a\"; s += \"b\"; return 0; }").unwrap_err();
    assert!(e.contains("requires a numeric target"), "{e}");
}

#[test]
fn arena_block_and_new_typechecks() {
    assert!(check(
        "struct Pt { x: int, y: int }
         fn main() -> int {
             arena(1024) {
                 p := new(Pt);
                 p.x = 10;
                 arr := new(int, 100);
                 arr[0] = p.x;
             }
             return 0;
         }"
    ).is_ok());

    // default arena capacity (no args)
    assert!(check(
        "fn main() -> int {
             arena {
                 x := new(int);
                 *x = 5;
             }
             return 0;
         }"
    ).is_ok());

    // wlel_arena() outside arena rejected
    let e = check("fn main() -> int { a := wlel_arena(); return 0; }").unwrap_err();
    assert!(e.contains("outside of an active arena"), "{e}");

    // new(void) rejected
    let e2 = check("fn main() -> int { p := new(void); return 0; }").unwrap_err();
    assert!(e2.contains("cannot allocate void"), "{e2}");
}

#[test]
fn for_loop_typechecks() {
    assert!(check(
        "fn main() -> int {
             sum := 0;
             for i in 0..10 {
                 sum += i;
             }
             return sum;
         }"
    ).is_ok());

    // non-int bounds rejected
    let e = check("fn main() -> int { for i in 1.5..10 { } return 0; }").unwrap_err();
    assert!(e.contains("bounds must be int"), "{e}");
}

#[test]
fn string_operations_typecheck() {
    assert!(check(
        "fn main() -> int {
             a := \"hello\";
             b := \"world\";
             c := a + \" \" + b;
             if c == \"hello world\" && a != b {
                 return 0;
             }
             return 1;
         }"
    ).is_ok());

    // relational on strings rejected
    let e = check("fn main() -> int { return \"a\" < \"b\"; }").unwrap_err();
    assert!(e.contains("relational '<, >, <=, >=' not supported on strings"), "{e}");
}

#[test]
fn sys_builtins_typecheck() {
    assert!(check(
        "fn main() -> int {
             if sys::argc() > 1 {
                 first := sys::arg(1);
                 wlel_print_str(first);
             }
             sys::exit(0);
             return 0;
         }"
    ).is_ok());

    let e = check("fn main() -> int { return sys::arg(\"bad\"); }").unwrap_err();
    assert!(e.contains("i must be int"), "{e}");
}

#[test]
fn literal_adapts_in_comparison() {
    // untyped literal compares against a narrower/wider variable
    assert!(check(
        "fn f(b: u8) -> bool { return b == 4; }"
    )
    .is_ok());
    assert!(check(
        "fn f(x: u64) -> bool { return x > 1000; }"
    )
    .is_ok());

    // out-of-range literal still caught on the adapted width
    let e = check("fn f(b: u8) -> bool { return b == 300; }").unwrap_err();
    assert!(e.contains("literal 300 does not fit in type u8"), "{e}");

    // negative literal vs unsigned still rejected
    let e2 = check("fn f(x: u64) -> bool { return x < -1; }").unwrap_err();
    assert!(e2.contains("literal -1 does not fit in type u64"), "{e2}");

    // two differently-typed variables still need a cast
    let e3 = check("fn f(a: u8, b: i32) -> bool { return a == b; }").unwrap_err();
    assert!(e3.contains("mixed int widths: u8 and i32"), "{e3}");
}

#[test]
fn unused_local_variable_warned() {
    let ws = check_warn(
        "fn main() -> int {
             x := 1;
             return 0;
         }",
    )
    .unwrap();
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert!(ws[0].msg.contains("unused variable 'x'"), "{ws:?}");
    assert_eq!(ws[0].span.start.line, 2);

    // read via compound assignment counts as use
    let ws2 = check_warn("fn main() -> int { x := 1; x += 2; return x; }").unwrap();
    assert!(ws2.is_empty(), "{ws2:?}");
}

#[test]
fn write_only_variable_warned() {
    // assigned but never read
    let ws = check_warn(
        "fn main() -> int {
             x := 0;
             x = 5;
             return 0;
         }",
    )
    .unwrap();
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert!(ws[0].msg.contains("unused variable 'x'"), "{ws:?}");

    // ...but `x = x + 1` reads x on the right-hand side — no warning
    // (assignment-flow analysis is out of scope for v1)
    let ws2 = check_warn("fn main() -> int { x := 0; x = x + 1; return 0; }").unwrap();
    assert!(ws2.is_empty(), "{ws2:?}");

    let ws3 = check_warn("fn main() -> int { x := 0; x = x + 1; return x; }").unwrap();
    assert!(ws3.is_empty(), "{ws3:?}");
}

#[test]
fn underscore_prefix_opts_out() {
    let ws = check_warn(
        "fn f(_p: int) -> int { return 0; }
         fn main() -> int {
             _x := 1;
             let r: int = f(2);
             return r;
         }",
    )
    .unwrap();
    assert!(ws.is_empty(), "{ws:?}");
}

#[test]
fn unused_parameter_warned() {
    let ws = check_warn("fn f(a: int, b: int) -> int { return a; } fn main() -> int { return f(1, 2); }")
        .unwrap();
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert!(ws[0].msg.contains("unused parameter 'b'"), "{ws:?}");
}

#[test]
fn unused_var_in_nested_scope_warned() {
    let ws = check_warn(
        "fn main() -> int {
             if true { y := 2; }
             return 0;
         }",
    )
    .unwrap();
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert!(ws[0].msg.contains("unused variable 'y'"), "{ws:?}");
}

#[test]
fn unreachable_after_terminator_warned() {
    let ws = check_warn(
        "fn f() -> int {
             return 1;
             return 2;
         }",
    )
    .unwrap();
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert_eq!(ws[0].msg, "unreachable statement", "{ws:?}");
    assert_eq!(ws[0].span.start.line, 3);

    // break/continue also terminate
    let ws2 = check_warn(
        "fn main() -> int {
             i := 0;
             while i < 3 {
                 break;
                 i += 1;
             }
             return i;
         }",
    )
    .unwrap();
    assert_eq!(ws2.len(), 1, "{ws2:?}");
    assert_eq!(ws2[0].msg, "unreachable statement", "{ws2:?}");
}

#[test]
fn code_after_if_return_is_reachable() {
    // a terminator inside a nested block must not leak into the outer block
    let ws = check_warn(
        "fn f(n: int) -> int {
             if n > 0 { return 1; }
             return 2;
         }",
    )
    .unwrap();
    assert!(ws.is_empty(), "{ws:?}");
}

#[test]
fn unused_std_import_warned() {
    // use std without any std:: call still type-checks; warnings only
    let ws = check_warn("use std;\nfn main() -> int { return 0; }").unwrap();
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert!(ws[0].msg.contains("import 'std' is never used"), "{ws:?}");
    assert_eq!(ws[0].span.start.line, 1);

    // used std import stays silent
    let ws2 = check_warn("use std;\nfn main() -> int { std::println_int(1); return 0; }").unwrap();
    assert!(ws2.is_empty(), "{ws2:?}");
}

#[test]
fn unused_file_import_warned() {
    let mut p = parse_ok("use \"geom.wl\";\nfn main() -> int { return 0; }");
    p.uses[0].resolved = Some("/proj/geom.wl".into());
    let ws = Checker::check(&mut p).unwrap();
    assert_eq!(ws.len(), 1, "{ws:?}");
    assert!(ws[0].msg.contains("import 'geom.wl' is never used"), "{ws:?}");

    // ...but a call into the imported file marks it used
    let mut p2 = parse_ok("use \"geom.wl\";\nfn main() -> int { return area(2); }");
    p2.uses[0].resolved = Some("/proj/geom.wl".into());
    for f in p2.funcs.iter_mut() {
        f.file = "/proj/main.wl".into();
    }
    let mut imported = parse_file_with_fn("/proj/geom.wl", "area");
    p2.funcs.append(&mut imported.funcs);
    let ws2 = Checker::check(&mut p2).unwrap();
    assert!(ws2.is_empty(), "{ws2:?}");
}

/// helper: parse a one-function program with the given origin file
fn parse_file_with_fn(file: &str, name: &str) -> wlel::ast::Program {
    let src = format!("fn {name}(x: int) -> int {{ return x; }}");
    let mut p = parse_ok(&src);
    for f in p.funcs.iter_mut() {
        f.file = file.into();
    }
    p
}

#[test]
fn warnings_do_not_fail_the_check() {
    // all-warning program still returns Ok
    let ws = check_warn(
        "use std;
         fn dead(_u: int) -> int { return 1; return 2; }
         fn main() -> int { x := dead(1); return 0; }",
    )
    .unwrap();
    // x unused, second return unreachable, and `use std` never used
    assert_eq!(ws.len(), 3, "{ws:?}");
}

#[test]
fn defer_block_typechecks() {
    assert!(check(
        "fn open() -> int { return 1; }
         fn close(fd: int) -> void { return; }
         fn main() -> int {
             defer {
                 let fd: int = open();
                 close(fd);
             }
             return 0;
         }"
    )
    .is_ok());

    // nested defer blocks allowed
    assert!(check(
        "fn f() -> void { return; }
         fn main() -> int {
             defer {
                 defer f();
             }
             return 0;
         }"
    )
    .is_ok());
}

#[test]
fn defer_block_forbids_control_flow() {
    let e = check("fn main() -> int { defer { return 1; } return 0; }").unwrap_err();
    assert!(e.contains("'return' inside a defer block is not allowed"), "{e}");

    let e2 = check(
        "fn main() -> int {
             while true {
                 defer { break; }
             }
             return 0;
         }",
    )
    .unwrap_err();
    assert!(e2.contains("'break' inside a defer block is not allowed"), "{e2}");

    let e3 = check(
        "fn main() -> int {
             while true {
                 defer { continue; }
             }
             return 0;
         }",
    )
    .unwrap_err();
    assert!(e3.contains("'continue' inside a defer block is not allowed"), "{e3}");
}

#[test]
fn checked_ops_typecheck() {
    assert!(check(
        "use std;
         fn main() -> int {
             r := 0;
             if std::checked_add(1, 2, &r) { return r; }
             return -1;
         }"
    )
    .is_ok());

    // third argument must be *int
    let e = check(
        "use std;
         fn main() -> int {
             r := 0;
             return std::checked_add(1, 2, r);
         }",
    )
    .unwrap_err();
    assert!(e.contains("std::checked_add argument 3: expected *int, got int"), "{e}");

    // pointer to the wrong width rejected
    let e2 = check(
        "use std;
         fn main() -> int {
             r := 0u8;
             return std::checked_add(1, 2, &r);
         }",
    )
    .unwrap_err();
    assert!(e2.contains("argument 3: expected *int, got *u8"), "{e2}");

    // operands must be int (no float, no mixed width)
    let e3 = check(
        "use std;
         fn main() -> int {
             r := 0;
             return std::checked_add(1.5, 2, &r);
         }",
    )
    .unwrap_err();
    assert!(e3.contains("argument 1: expected int, got float"), "{e3}");

    let e4 = check(
        "use std;
         fn main() -> int {
             r := 0;
             return std::checked_mul(1, 2u8, &r);
         }",
    )
    .unwrap_err();
    assert!(e4.contains("argument 2: expected int, got u8"), "{e4}");

    // argument count
    let e5 = check("use std;\nfn main() -> int { r := 0; return std::checked_sub(1, &r); }")
        .unwrap_err();
    assert!(e5.contains("std::checked_sub takes 3 argument(s), got 2"), "{e5}");
}
