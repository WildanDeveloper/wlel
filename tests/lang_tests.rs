use wlel::ast::*;
use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::token::Token;
use wlel::parser::Parser;

fn parse(src: &str) -> Program {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
    p
}

/// generated C with `#line` directives removed, so multi-statement
/// substring assertions don't break on interleaved diagnostics
fn c_no_line(c: &str) -> String {
    c.lines()
        .filter(|l| !l.trim_start().starts_with("#line"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn fib_program() {
    let p = parse(
        "fn fib(n: int) -> int {
            if n < 2 { return n; }
            return fib(n - 1) + fib(n - 2);
        }",
    );
    assert_eq!(p.funcs.len(), 1);
    let f = &p.funcs[0];
    assert_eq!(f.name, "fib");
    assert_eq!(f.params.len(), 1);
    assert_eq!(f.params[0].name, "n");
    assert_eq!(f.params[0].ty.as_deref(), Some("int"));
    assert_eq!(f.ret_type.as_deref(), Some("int"));
    assert_eq!(f.body.0.len(), 2);
    match &f.body.0[0].node {
        StmtKind::If(i) => match &i.cond.node {
            ExprKind::Binary(BinOp::Lt, l, r) => {
                assert!(matches!(l.node, ExprKind::Ident(ref n) if n == "n"));
                assert!(matches!(r.node, ExprKind::Int(2)));
            }
            other => panic!("expected n < 2, got {:?}", other),
        },
        other => panic!("expected if, got {:?}", other),
    }
}

#[test]
fn precedence_mul_over_add() {
    let p = parse("fn t() -> int { return 1 + 2 * 3; }");
    match &p.funcs[0].body.0[0].node {
        StmtKind::Return(Some(e)) => match &e.node {
            ExprKind::Binary(BinOp::Add, l, r) => {
                assert!(matches!(l.node, ExprKind::Int(1)));
                match &r.node {
                    ExprKind::Binary(BinOp::Mul, rl, rr) => {
                        assert!(matches!(rl.node, ExprKind::Int(2)));
                        assert!(matches!(rr.node, ExprKind::Int(3)));
                    }
                    other => panic!("{:?}", other),
                }
            }
            other => panic!("{:?}", other),
        },
        other => panic!("{:?}", other),
    }
}

#[test]
fn left_assoc_sub() {
    let p = parse("fn t() -> int { return 10 - 4 - 3; }");
    match &p.funcs[0].body.0[0].node {
        StmtKind::Return(Some(e)) => match &e.node {
            ExprKind::Binary(BinOp::Sub, l, r) => {
                assert!(matches!(r.node, ExprKind::Int(3)));
                match &l.node {
                    ExprKind::Binary(BinOp::Sub, ll, lr) => {
                        assert!(matches!(ll.node, ExprKind::Int(10)));
                        assert!(matches!(lr.node, ExprKind::Int(4)));
                    }
                    other => panic!("{:?}", other),
                }
            }
            other => panic!("{:?}", other),
        },
        other => panic!("{:?}", other),
    }
}

#[test]
fn assign_vs_eq_lookahead() {
    let p = parse(
        "fn t() -> int {
            x := 1;
            x = x == 1;
            foo(x);
            while x < 10 { x := x + 1; }
            return 0;
        }",
    );
    let body = &p.funcs[0].body.0;
    assert!(matches!(&body[0].node, StmtKind::Let(n, _, _) if n == "x"));
    match &body[1].node {
        StmtKind::Assign(a) => assert!(matches!(a.target.node, ExprKind::Ident(ref n) if n == "x")),
        other => panic!("{:?}", other),
    }
    assert!(matches!(&body[2].node, StmtKind::ExprStmt(e) if matches!(e.node, ExprKind::Call(ref f, ref args) if f == "foo" && args.len() == 1)));
    assert!(matches!(&body[3].node, StmtKind::While(..)));
}

#[test]
fn else_if_chain() {
    let p = parse(
        "fn g(a: int) -> int {
            if a == 1 { return 1; } else if a == 2 { return 2; } else { return 3; }
        }",
    );
    match &p.funcs[0].body.0[0].node {
        StmtKind::If(i) => match &i.else_branch {
            Some(ElseBranch::If(inner)) => assert!(inner.else_branch.is_some()),
            other => panic!("{:?}", other),
        },
        other => panic!("{:?}", other),
    }
}

#[test]
fn syntax_error_reported() {
    let toks = Lexer::new("fn (").tokenize().unwrap();
    let (_, errs) = Parser::new(&toks).program();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].msg.contains("expected identifier"), "{errs:?}");
    // error points at the '(' token: line 1, col 4
    assert_eq!(errs[0].span.start.line, 1);
    assert_eq!(errs[0].span.start.col, 4);
}

#[test]
fn syntax_error_span_multi_line() {
    let toks = Lexer::new("fn t() -> int {\n    return 0;\n}\nlet x\n").tokenize().unwrap();
    let (_, errs) = Parser::new(&toks).program();
    assert_eq!(errs[0].span.start.line, 4);
}

#[test]
fn two_syntax_errors_both_reported() {
    let toks = Lexer::new(
        "fn main() -> int {
             x := ;
             y := 2
             return 0;
         }",
    )
    .tokenize()
    .unwrap();
    let (p, errs) = Parser::new(&toks).program();
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert_eq!(errs[0].span.start.line, 2, "{errs:?}"); // `x := ;`
    // error points at the unexpected token (`return`), per `expect` style
    assert_eq!(errs[1].span.start.line, 4, "{errs:?}");
    // recovery kept going: the broken statements are dropped whole,
    // the intact `return 0;` survived
    assert_eq!(p.funcs[0].body.0.len(), 1, "{:?}", p.funcs[0].body.0);
}

#[test]
fn recovery_continues_across_functions() {
    let toks = Lexer::new(
        "fn broken( {
         }
         fn good() -> int { return 1; }
         fn main() -> int { return good(); }",
    )
    .tokenize()
    .unwrap();
    let (p, errs) = Parser::new(&toks).program();
    assert_eq!(errs.len(), 1, "{errs:?}");
    // both intact functions survived the broken one
    assert_eq!(p.funcs.len(), 2, "{:?}", p.funcs.iter().map(|f| &f.name).collect::<Vec<_>>());
    assert_eq!(p.funcs[0].name, "good");
}

#[test]
fn missing_closing_brace_reports_once() {
    let toks = Lexer::new("fn f() -> int { return 1;\n").tokenize().unwrap();
    let (_, errs) = Parser::new(&toks).program();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].msg.contains("RBrace"), "{errs:?}");
}

#[test]
fn codegen_fib_emits_c() {
    let c = gen_program(&parse(
        "fn fib(n: int) -> int {
            if n < 2 { return n; }
            return fib(n - 1) + fib(n - 2);
        }
        fn main() -> int { wlel_print_int(fib(10)); return 0; }",
    ));
    assert!(c.contains("long long fib(long long n)"), "{c}");
    assert!(c.contains("return n;"), "{c}");
    assert!(c.contains("fib((n - 1)) + fib((n - 2))"), "{c}");
}

#[test]
fn strings_with_escapes_and_utf8() {
    let src = r##""hi\nthere \"x\" 🇮🇩""##;
    let toks = Lexer::new(src).tokenize().expect("should lex");
    assert_eq!(toks[0], Token::Str("hi\nthere \"x\" 🇮🇩".into()));
}

#[test]
fn struct_def_and_pointer_ops_parse() {
    let p = parse(
        "struct Node {
            value: int,
            next: *Node
        }
        fn main() -> int {
            let n: Node = Node { value: 1, next: 0 };
            let p: *Node = &n;
            p.value = 2;
            (*p).value = p.value + 1;
            return n.value;
        }",
    );
    assert_eq!(p.structs.len(), 1);
    let st = &p.structs[0];
    assert_eq!(st.name, "Node");
    assert_eq!(st.fields[1].1, "*Node");

    match &p.funcs[0].body.0[2].node {
        StmtKind::Assign(a) => match &a.target.node {
            ExprKind::Field(_, f) => assert_eq!(f, "value"),
            other => panic!("{:?}", other),
        },
        other => panic!("{:?}", other),
    }
}

#[test]
fn codegen_emits_structs_and_pointers() {
    let c = gen_program(&parse(
        "struct Pt { x: float, y: float }
         fn bump(p: *Pt) -> void { p.x = p.x + 1.0; }
         fn main() -> int { let o: Pt = Pt { x: 0.0, y: 2.5 }; bump(&o); return 0; }",
    ));
    assert!(c.contains("typedef struct Pt Pt;"), "{c}");
    assert!(c.contains("struct Pt {"), "{c}");
    assert!(c.contains("double x;"), "{c}");
    assert!(c.contains("static void bump(Pt* p);"), "{c}");
    assert!(c.contains("(Pt){ .x = 0.0, .y = 2.5 }"), "{c}");
    assert!(c.contains("p.x = (p.x + 1.0);"), "{c}");
    assert!(c.contains("bump(&o);"), "{c}");
}

#[test]
fn defer_lifo_and_early_return_codegen() {
    let c = gen_program(&parse(
        "fn main() -> int {
            defer cleanup(1);
            defer cleanup(2);
            if true { return 0; }
            return 1;
        }
        fn cleanup(n: int) -> void { wlel_print_int(n); }",
    ));
    // early return inside defer-having fn: expr evaluated first, then defers (LIFO)
    assert!(c.contains("int _wlel_ret = 0;\n            cleanup(2);\n            cleanup(1);\n            return _wlel_ret;"), "{c}");
    // block end: reversed order
    assert!(c.contains("cleanup(2);\n    cleanup(1);\n}"), "{c}");
}

#[test]
fn arrays_casts_and_arena_codegen() {
    let c = gen_program(&parse(
        "struct Pt { x: int }
         fn main() -> int {
             let a: [int; 3] = [1, 2, 3];
             a[0] = 9;
             let p: *Pt = wlel_alloc(wlel_sizeof(Pt)) as *Pt;
             p.x = 5;
             wlel_free(p);
             return a[0];
         }",
    ));
    assert!(c.contains("long long a[3] = { 1, 2, 3 };"), "{c}");
    assert!(c.contains("a[0] = 9;"), "{c}");
    assert!(c.contains("Pt* p = (Pt*)(wlel_alloc(sizeof(Pt)));"), "{c}");
    assert!(c.contains("p.x = 5;"), "{c}");
    assert!(c.contains("wlel_free(p);"), "{c}");
}

#[test]
fn compound_assign_codegen() {
    let c = gen_program(&parse(
        "fn main() -> int {
            x := 1;
            x += 2;
            x -= 1;
            x *= 4;
            x /= 2;
            x %= 3;
            return x;
        }",
    ));
    assert!(c.contains("x += 2;"), "{c}");
    assert!(c.contains("x -= 1;"), "{c}");
    assert!(c.contains("x *= 4;"), "{c}");
    assert!(c.contains("x /= 2;"), "{c}");
    assert!(c.contains("x %= 3;"), "{c}");
}

#[test]
fn bitwise_ops_codegen() {
    let c = gen_program(&parse(
        "fn main() -> int {
            a := 0xF0;
            b := 0x0F;
            return (a & b) | (a ^ b) | (1 << 4) | (256 >> 4) | ~0;
        }",
    ));
    assert!(c.contains("((a & b) | (a ^ b))"), "{c}");
    assert!(c.contains("(1 << 4)"), "{c}");
    assert!(c.contains("(256 >> 4)"), "{c}");
    assert!(c.contains("(~0)"), "{c}");
}

#[test]
fn break_continue_and_loop_defers_codegen() {
    let c = gen_program(&parse(
        "fn tick() -> void { wlel_print_str(\"t\"); }
         fn main() -> int {
             i := 0;
             while i < 10 {
                 defer tick();
                 i += 1;
                 if i == 2 { continue; }
                 if i == 5 { break; }
             }
             return i;
         }",
    ));
    // continue/break run loop-body defers registered so far (LIFO) before jumping
    assert!(c.contains("{\n                tick();\n                continue;\n            }"), "{c}");
    assert!(c.contains("{\n                tick();\n                break;\n            }"), "{c}");
}

#[test]
fn void_main_emits_return_zero() {
    let c = gen_program(&parse(
        "fn main() { wlel_print_str(\"hi\"); return; }",
    ));
    assert!(c.contains("int main("), "{c}");
    assert!(c.contains("return 0;"), "{c}");
    assert!(!c.contains("return;"), "{c}");

    // without an explicit return the synthesized one closes main
    let c2 = gen_program(&parse("fn main() { wlel_print_str(\"hi\"); }"));
    assert!(c2.contains("return 0;\n}"), "{c2}");
    // and `return;` becomes `return 0;`
    let c3 = gen_program(&parse("fn main() { wlel_print_str(\"hi\"); return; }"));
    assert!(c3.contains("return 0;\n}"), "{c3}");
}

#[test]
fn radix_and_scientific_literals() {
    let toks = Lexer::new("0xFF 0b1010 0o17 1_000_000 1.5e3 2e-2 1E+10")
        .tokenize()
        .expect("lex ok");
    assert_eq!(toks[0], Token::Int(255));
    assert_eq!(toks[1], Token::Int(10));
    assert_eq!(toks[2], Token::Int(15));
    assert_eq!(toks[3], Token::Int(1_000_000));
    assert_eq!(toks[4], Token::Float(1500.0));
    assert_eq!(toks[5], Token::Float(0.02));
    assert_eq!(toks[6], Token::Float(1e10));
    assert_eq!(toks[7], Token::Eof);
}

#[test]
fn precedence_bitwise_between_logic_and_cmp() {
    let p = parse("fn t() -> int { return 1 | 2 ^ 3 & 4 == 0; }");
    // C-style: & tighter than ^ tighter than | ; == tighter than &
    match &p.funcs[0].body.0[0].node {
        StmtKind::Return(Some(e)) => match &e.node {
            ExprKind::Binary(BinOp::BitOr, _, rhs) => match &rhs.node {
                ExprKind::Binary(BinOp::BitXor, _, xr) => match &xr.node {
                    ExprKind::Binary(BinOp::BitAnd, _, ar) => match &ar.node {
                        ExprKind::Binary(BinOp::Eq, ..) => {}
                        other => panic!("expected == innermost, got {:?}", other),
                    },
                    other => panic!("expected & next, got {:?}", other),
                },
                other => panic!("expected ^ next, got {:?}", other),
            },
            other => panic!("{:?}", other),
        },
        other => panic!("{:?}", other),
    }
}

#[test]
fn arena_block_codegen() {
    let c = gen_program(&parse(
        "struct Pt { x: int }
         fn main() -> int {
             arena(1024) {
                 p := new(Pt);
                 p.x = 5;
                 arr := new(int, 10);
                 arr[0] = 1;
             }
             q := new(Pt);
             q.x = 7;
             return 0;
         }",
    ));
    // arena block: save current, create new, switch
    assert!(c.contains("WArena* _wlel_arena_prev = _wlel_cur_arena;"), "{c}");
    assert!(c.contains("WArena* _wlel_arena_cur = wlel_arena_new(1024);"), "{c}");
    assert!(c.contains("_wlel_cur_arena = _wlel_arena_cur;"), "{c}");
    // new() inside arena uses arena_alloc
    assert!(c.contains("(Pt*)(_wlel_cur_arena ? wlel_arena_alloc(_wlel_cur_arena, sizeof(Pt)) : malloc(sizeof(Pt)))"), "{c}");
    assert!(c.contains("wlel_arena_alloc(_wlel_cur_arena, sizeof(long long) * (10))"), "{c}");
    // new() outside arena falls back to malloc
    assert!(c.contains("(Pt*)(_wlel_cur_arena ? wlel_arena_alloc(_wlel_cur_arena, sizeof(Pt)) : malloc(sizeof(Pt)))"), "{c}");
    // defer-registered exit frees the arena (registered before body runs last)
    assert!(c.contains("__wlel_arena_exit(_wlel_arena_prev);"), "{c}");
}

#[test]
fn arena_early_return_frees() {
    let c = gen_program(&parse(
        "fn f(x: int) -> int {
             arena(1024) {
                 p := new(int);
                 if x > 0 { return 1; }
             }
             return 0;
         }
         fn main() -> int { return f(1); }",
    ));
    // early return inside arena must evaluate the return value first,
    // then free the arena, then return the saved value
    assert!(c.contains("long long _wlel_ret = 1;\n                __wlel_arena_exit(_wlel_arena_prev);\n                return _wlel_ret;"), "{c}");
}

#[test]
fn arena_nested_and_current_arena_codegen() {
    let c = gen_program(&parse(
        "fn peek(a: *void) -> int { return 0; }
         fn main() -> int {
             arena {
                 inner := wlel_arena();
                 arena {
                     deeper := wlel_arena();
                 }
             }
             return 0;
         }",
    ));
    // two nested arena blocks, restore chain via prev pointer
    assert_eq!(c.matches("wlel_arena_new(4096)").count(), 2, "{c}");
    assert!(c.contains("(void*)_wlel_cur_arena"), "{c}");
    assert_eq!(c.matches("__wlel_arena_exit(_wlel_arena_prev);").count(), 2, "{c}");
}

#[test]
fn for_loop_codegen() {
    let c = gen_program(&parse(
        "fn main() -> int {
             sum := 0;
             for i in 0..10 {
                 sum += i;
             }
             return sum;
         }",
    ));
    assert!(c.contains("for (long long i = 0; i < 10; i += 1) {"), "{c}");
}

#[test]
fn for_loop_with_defers_and_continue_codegen() {
    let c = gen_program(&parse(
        "fn tick() -> void { wlel_print_str(\"t\"); }
         fn main() -> int {
             for i in 0..10 {
                 defer tick();
                 if i == 3 { continue; }
             }
             return 0;
         }",
    ));
    // C for-loop: continue naturally runs the increment
    assert!(c.contains("for (long long i = 0; i < 10; i += 1) {"), "{c}");
    assert!(c.contains("continue;"), "{c}");
}

#[test]
fn string_concat_and_eq_codegen() {
    let mut p = parse(
        "fn main() -> int {
             a := \"hel\";
             b := \"lo\";
             wlel_print_str(a + b);
             if a + b == \"hello\" { return 1; }
             if a != b { return 2; }
             return 0;
         }",
    );
    Checker::check(&mut p).expect("typecheck");
    let c = gen_program(&p);
    assert!(c.contains("_wlel_strcat(a, b)"), "{c}");
    assert!(c.contains("_wlel_streq(_wlel_strcat(a, b), \"hello\")"), "{c}");
    assert!(c.contains("(!_wlel_streq(a, b))"), "{c}");
}

#[test]
fn defer_block_codegen_multi_statement() {
    let c = gen_program(&parse(
        "fn cleanup(n: int) -> void { wlel_print_int(n); }
         fn log() -> void { wlel_print_str(\"log\"); }
         fn main() -> int {
             defer cleanup(1);
             defer {
                 cleanup(2);
                 cleanup(3);
             }
             defer cleanup(4);
             return 0;
         }",
    ));
    // scope exit: LIFO across both forms — 4, block(2,3), 1
    let c = c_no_line(&c);
    assert!(c.contains("cleanup(4);\n    {\n        cleanup(2);\n        cleanup(3);\n    }\n    cleanup(1);"), "{c}");
}

#[test]
fn defer_block_runs_on_early_return() {
    let c = gen_program(&parse(
        "fn cleanup(n: int) -> void { wlel_print_int(n); }
         fn f(x: int) -> int {
             defer {
                 cleanup(9);
             }
             if x > 0 { return 1; }
             return 0;
         }",
    ));
    // the deferred block runs before the saved return value is returned
    let c = c_no_line(&c);
    assert!(c.contains("long long _wlel_ret = 1;\n            {\n                cleanup(9);\n            }\n            return _wlel_ret;"), "{c}");
}

#[test]
fn defer_block_inner_defer_runs_after_block() {
    let c = gen_program(&parse(
        "fn a() -> void { }
         fn b() -> void { }
         fn main() -> int {
             defer {
                 a();
                 defer b();
             }
             return 0;
         }",
    ));
    // the block's statements run, then the inner defer at the end of the
    // SAME block (a deferred block owns its defer scope)
    let c = c_no_line(&c);
    assert!(c.contains("{\n            a();\n            b();\n        }"), "{c}");
}

#[test]
fn sys_args_codegen() {
    let c = gen_program(&parse(
        "fn main() -> int {
             wlel_print_int(sys::argc());
             wlel_print_str(sys::arg(0));
             return 0;
         }",
    ));
    assert!(c.contains("int main(int _wlel_main_argc, char** _wlel_main_argv) {"), "{c}");
    assert!(c.contains("_wlel_argc = _wlel_main_argc; _wlel_argv = _wlel_main_argv;"), "{c}");
    assert!(c.contains("_wlel_root_arena = wlel_arena_new(65536);"), "{c}");
    assert!(c.contains("sys__argc()"), "{c}");
    assert!(c.contains("sys__arg(0)"), "{c}");
}

#[test]
fn line_directives_emitted_in_c() {
    let mut p = parse(
        "fn calculate(a: int) -> int {
             x := a * 2;
             return x + 1;
         }",
    );
    p.funcs[0].file = "calc.wl".into();
    let c = gen_program(&p);
    assert!(c.contains("#line 1 \"calc.wl\""), "{c}");
    assert!(c.contains("#line 2 \"calc.wl\""), "{c}");
    assert!(c.contains("#line 3 \"calc.wl\""), "{c}");
}

#[test]
fn width_suffix_codegen() {
    let c = gen_program(&parse(
        "fn main() -> int {
             x := 10u8;
             y := 100_000i32;
             z := 2.5f32;
             w := 7usize;
             return 0;
         }",
    ));
    assert!(c.contains("uint8_t x = (uint8_t)(10);"), "{c}");
    assert!(c.contains("int32_t y = (int32_t)(100000);"), "{c}");
    assert!(c.contains("float z = (float)(2.5);"), "{c}");
    assert!(c.contains("size_t w = (size_t)(7);"), "{c}");
}

#[test]
fn checked_ops_runtime_builtins_emitted() {
    let c = gen_program(&parse(
        "fn main() -> int {
             r := 0;
             if std::checked_add(1, 2, &r) { return r; }
             if std::checked_mul(3, 4, &r) { return r; }
             return 0;
         }",
    ));
    assert!(c.contains("static _Bool std__checked_add(long long a, long long b, long long* out) { return !__builtin_add_overflow(a, b, out); }"), "{c}");
    assert!(c.contains("static _Bool std__checked_sub(long long a, long long b, long long* out) { return !__builtin_sub_overflow(a, b, out); }"), "{c}");
    assert!(c.contains("static _Bool std__checked_mul(long long a, long long b, long long* out) { return !__builtin_mul_overflow(a, b, out); }"), "{c}");
    assert!(c.contains("std__checked_add(1, 2, &r)"), "{c}");
    assert!(c.contains("std__checked_mul(3, 4, &r)"), "{c}");
}

#[test]
fn i64_wrap_add_codegen_stays_plain_c() {
    // signed wrap is guaranteed by -fwrapv at cc time; generated C stays
    // clean and readable (no explicit builtins for ordinary arithmetic)
    let c = gen_program(&parse(
        "fn main() -> int {
             max := 9_223_372_036_854_775_807;
             let wrapped: int = max + 1;
             return wrapped;
         }",
    ));
    assert!(c.contains("long long wrapped = (max + 1);"), "{c}");
}
