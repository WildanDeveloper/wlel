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

#[test]
fn test_block_body_typechecks() {
    assert!(check(
        "fn add(a: int, b: int) -> int { return a + b; }
         test \"works\" {
             assert_eq(add(1, 1), 2);
             assert(true);
         }"
    )
    .is_ok());
}

#[test]
fn test_body_can_call_and_be_checked() {
    // undefined name inside a test body is an error, like anywhere else
    let e = check("test \"bad\" { return nope; }").unwrap_err();
    assert!(e.contains("undefined variable 'nope'"), "{e}");
}

#[test]
fn test_cannot_return_a_value() {
    let e = check("test \"x\" { return 5; }").unwrap_err();
    assert!(e.contains("void function cannot return a value"), "{e}");
}

#[test]
fn test_break_outside_loop_rejected() {
    let e = check("test \"x\" { break; }").unwrap_err();
    assert!(e.contains("'break' outside of loop"), "{e}");
}

#[test]
fn assert_needs_bool() {
    let e = check("fn main() -> int { assert(1); return 0; }").unwrap_err();
    assert!(e.contains("assert() expects a bool condition, got int"), "{e}");
    let e2 = check("fn main() -> int { assert(); return 0; }").unwrap_err();
    assert!(e2.contains("assert() takes exactly 1 argument"), "{e2}");
}

#[test]
fn assert_eq_semantics_match_eq_operator() {
    // untyped literal adapts to the variable's width (range-checked)
    assert!(check(
        "fn main() -> int {
             let b: u8 = 5;
             assert_eq(b, 5);
             return 0;
         }"
    )
    .is_ok());
    // literal out of range for the target width
    let e = check(
        "fn main() -> int {
             let b: u8 = 5;
             assert_eq(b, 300);
             return 0;
         }",
    )
    .unwrap_err();
    assert!(e.contains("literal 300 does not fit in type u8"), "{e}");
    // mixed int widths without a cast
    let e2 = check(
        "fn main() -> int {
             let a: u8 = 5;
             let b: i32 = 6;
             assert_eq(a, b);
             return 0;
         }",
    )
    .unwrap_err();
    assert!(e2.contains("mixed int widths: u8 and i32"), "{e2}");
    // mixed float widths
    let e3 = check(
        "fn main() -> int {
             let a: f32 = 1.0;
             let b: float = 1.0;
             assert_eq(a, b);
             return 0;
         }",
    )
    .unwrap_err();
    assert!(e3.contains("comparison between f32 and float"), "{e3}");
    // unsupported operand kinds
    let e4 = check(
        "struct Pt { x: int }
         fn main() -> int {
             let p: Pt = Pt { x: 1 };
             assert_eq(p, p);
             return 0;
         }",
    )
    .unwrap_err();
    assert!(e4.contains("assert_eq() supports numbers, bools and strings"), "{e4}");
    // argument count
    let e5 = check("fn main() -> int { assert_eq(1); return 0; }").unwrap_err();
    assert!(e5.contains("assert_eq() takes exactly 2 arguments"), "{e5}");
}

#[test]
fn assert_works_in_plain_functions_too() {
    // assert is not test-only: outside a test run a failed assert exits(1)
    assert!(check("fn main() -> int { assert(1 == 1); return 0; }").is_ok());
    let e = check("fn main() -> int { assert(\"x\"); return 0; }").unwrap_err();
    assert!(e.contains("assert() expects a bool condition, got string"), "{e}");
}

#[test]
fn arena_stats_typechecks_and_exposes_fields() {
    assert!(check(
        "fn show(s: ArenaStats) -> int { return s.bytes + s.chunks + s.peak; }
         fn main() -> int {
             let s: ArenaStats = arena_stats();
             arena(1024) { p := new(int, 4); p[0] = 1; show(arena_stats()); }
             return show(s);
         }"
    ).is_ok());
}

#[test]
fn arena_stats_rejects_arguments() {
    assert!(check("fn main() -> int { arena_stats(1); return 0; }").is_err());
}

#[test]
fn arena_stats_rejects_bad_field() {
    assert!(check("fn main() -> int { return arena_stats().nope; }").is_err());
}

#[test]
fn arena_stats_struct_name_is_reserved() {
    let e = check_err("struct ArenaStats { x: int }
                       fn main() -> int { return 0; }")
        .expect_err("must be rejected");
    assert!(e.msg.contains("reserved"), "{}", e.msg);
}

#[test]
fn arena_stats_codegen_emits_helper_and_struct() {
    let mut p = parse_ok(
        "fn main() -> int {
             let s: ArenaStats = arena_stats();
             return s.bytes;
         }",
    );
    Checker::check(&mut p).expect("typecheck");
    let c = gen_program(&p);
    assert!(c.contains("typedef struct ArenaStats { long long bytes; long long chunks; long long peak; } ArenaStats;"), "{c}");
    assert!(c.contains("_wlel_arena_stats()"), "{c}");
    assert!(c.contains("s.peak = (long long)_wlel_cur_arena->peak;"), "{c}");
}

// ---------------------------------------------------------------------------
// parser recursion guard: hostile nesting must be rejected with a syntax
// error, never overflow the stack (nightly cargo-fuzz found this via a
// mutated defer.wl with ~900 nested parens)

fn parse_errors_of(src: String) -> Vec<String> {
    let toks = Lexer::new(&src).tokenize().expect("lex");
    let (_, errs) = Parser::new(&toks).program();
    errs.iter().map(|e| e.msg.clone()).collect()
}

#[test]
fn deep_paren_nesting_is_rejected_not_crashed() {
    let errs = parse_errors_of(format!("fn f() {{ x := {}1; }}", "(".repeat(5_000)));
    assert!(!errs.is_empty(), "deep parens must produce errors");
    assert!(
        errs.iter().any(|m| m.contains("expression nesting too deep")),
        "{errs:?}"
    );
}

#[test]
fn deep_unary_chain_is_rejected_not_crashed() {
    let errs = parse_errors_of(format!("fn f() {{ x := {}1; }}", "-".repeat(50_000)));
    assert!(
        errs.iter().any(|m| m.contains("expression nesting too deep")),
        "{errs:?}"
    );
}

#[test]
fn deep_block_nesting_is_rejected_not_crashed() {
    let src = format!("fn f() {{ {} let a := 1; {} }}", "{".repeat(50_000), "}".repeat(50_000));
    let errs = parse_errors_of(src);
    assert!(
        errs.iter().any(|m| m.contains("block nesting too deep")),
        "{errs:?}"
    );
}

#[test]
fn deep_type_nesting_is_rejected_not_crashed() {
    let errs = parse_errors_of(format!("fn f(x: {}int) {{ return 0; }}", "[".repeat(5_000)));
    assert!(
        errs.iter().any(|m| m.contains("type nesting too deep")),
        "{errs:?}"
    );
}

#[test]
fn nesting_under_the_limit_still_parses() {
    // 100 paren levels (200 counted bumps: expr+unary per level) — legal
    let src = format!("fn f() -> int {{ return {}1{}; }}", "(".repeat(100), ")".repeat(100));
    let errs = parse_errors_of(src);
    assert!(errs.is_empty(), "100-deep parens must parse: {errs:?}");
}
