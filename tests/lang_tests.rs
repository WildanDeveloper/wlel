use wlel::ast::*;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::token::Token;
use wlel::parser::Parser;

fn parse(src: &str) -> Program {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    Parser::new(&toks).program().expect("parse ok")
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
    match &f.body.0[0] {
        Stmt::If(i) => assert_eq!(
            i.cond,
            Expr::Binary(BinOp::Lt, Box::new(Expr::Ident("n".into())), Box::new(Expr::Int(2)))
        ),
        other => panic!("expected if, got {:?}", other),
    }
}

#[test]
fn precedence_mul_over_add() {
    let p = parse("fn t() -> int { return 1 + 2 * 3; }");
    match &p.funcs[0].body.0[0] {
        Stmt::Return(Some(e)) => assert_eq!(
            e,
            &Expr::Binary(
                BinOp::Add,
                Box::new(Expr::Int(1)),
                Box::new(Expr::Binary(BinOp::Mul, Box::new(Expr::Int(2)), Box::new(Expr::Int(3))))
            )
        ),
        other => panic!("{:?}", other),
    }
}

#[test]
fn left_assoc_sub() {
    let p = parse("fn t() -> int { return 10 - 4 - 3; }");
    match &p.funcs[0].body.0[0] {
        Stmt::Return(Some(e)) => assert_eq!(
            e,
            &Expr::Binary(
                BinOp::Sub,
                Box::new(Expr::Binary(BinOp::Sub, Box::new(Expr::Int(10)), Box::new(Expr::Int(4)))),
                Box::new(Expr::Int(3))
            )
        ),
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
    assert!(matches!(&body[0], Stmt::Let(n, _, _) if n == "x"));
    match &body[1] {
        Stmt::Assign(a) => assert!(matches!(a.target, Expr::Ident(ref n) if n == "x")),
        other => panic!("{:?}", other),
    }
    assert!(matches!(&body[2], Stmt::ExprStmt(Expr::Call(f, args)) if f == "foo" && args.len() == 1));
    assert!(matches!(&body[3], Stmt::While(..)));
}

#[test]
fn else_if_chain() {
    let p = parse(
        "fn g(a: int) -> int {
            if a == 1 { return 1; } else if a == 2 { return 2; } else { return 3; }
        }",
    );
    match &p.funcs[0].body.0[0] {
        Stmt::If(i) => match &i.else_branch {
            Some(ElseBranch::If(inner)) => assert!(inner.else_branch.is_some()),
            other => panic!("{:?}", other),
        },
        other => panic!("{:?}", other),
    }
}

#[test]
fn syntax_error_reported() {
    let toks = Lexer::new("fn (").tokenize().unwrap();
    let err = Parser::new(&toks).program().unwrap_err();
    assert!(err.msg.contains("expected identifier"));
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

    match &p.funcs[0].body.0[2] {
        Stmt::Assign(a) => match &a.target {
            Expr::Field(_, f) => assert_eq!(f, "value"),
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
    assert!(c.contains("typedef struct {"), "{c}");
    assert!(c.contains("double x;"), "{c}");
    assert!(c.contains("} Pt;"), "{c}");
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
    // early return inside defer-having fn: defers run before the return
    assert!(c.contains("{ cleanup(1); cleanup(2); return 0; }"), "{c}");
    // block end: reversed order
    assert!(c.contains("cleanup(2);\n    cleanup(1);\n}"), "{c}");
}
