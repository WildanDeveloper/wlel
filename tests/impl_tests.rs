//! Method sugar (`impl` blocks + `recv.method(args)`): parsing, the
//! checker's desugaring into plain functions, receiver adaptation
//! (auto-deref / auto-borrow), generic impls, error cases, fmt, and the
//! emitted C shape.

use std::fs;
use std::process::Command;

use wlel::ast::ExprKind;
use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::fmt::format_source;
use wlel::lexer::Lexer;
use wlel::parser::Parser;
use wlel::stdsrc::splice_std;

fn parse_ok(src: &str) -> wlel::ast::Program {
    let toks = Lexer::new(src).tokenize().expect("lex");
    let (p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
    p
}

fn front(src: &str) -> Result<String, String> {
    let mut p = parse_ok(src);
    splice_std(&mut p);
    for f in p.funcs.iter_mut() {
        if f.file.is_empty() {
            f.file = "methods.wl".into();
        }
    }
    for i in p.impls.iter_mut() {
        if i.file.is_empty() {
            i.file = "methods.wl".into();
        }
    }
    Checker::check(&mut p).map_err(|e| e.msg)?;
    Ok(gen_program(&p))
}

fn check_err_msg(src: &str) -> String {
    front(src).expect_err("must be rejected")
}

/// run a source file through `wlel run`
fn run_cli(name: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_impl_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join(format!("{name}.wl"));
    fs::write(&path, src).expect("write src");
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    let o = String::from_utf8_lossy(&out.stdout).into_owned();
    let e = String::from_utf8_lossy(&out.stderr).into_owned();
    let _ = fs::remove_dir_all(&dir);
    (out.status.success(), o, e)
}

// ---------------------------------------------------------------------------
// parsing

#[test]
fn parses_impl_block_and_method_call() {
    let p = parse_ok(
        "struct Pt { x: float, y: float }
         impl Pt {
             fn len(self) -> float { return self.x; }
             fn scale(self: *Pt, k: float) { self.x *= k; }
         }
         fn main() -> int { p := Pt { x: 1.0, y: 2.0 }; p.len(); return 0; }",
    );
    assert_eq!(p.impls.len(), 1);
    let imp = &p.impls[0];
    assert_eq!(imp.type_name, "Pt");
    assert!(imp.type_params.is_empty());
    assert_eq!(imp.methods.len(), 2);
    assert_eq!(imp.methods[0].name, "len");
    assert_eq!(imp.methods[1].name, "scale");

    // the call is a MethodCall node
    let mut found = false;
    for s in &p.funcs.last().unwrap().body.0 {
        if let wlel::ast::StmtKind::ExprStmt(e) = &s.node {
            if let ExprKind::MethodCall(recv, name, args) = &e.node {
                assert!(matches!(recv.node, ExprKind::Ident(ref n) if n == "p"));
                assert_eq!(name, "len");
                assert!(args.is_empty());
                found = true;
            }
        }
    }
    assert!(found, "method call parsed");
}

#[test]
fn parses_chained_methods_and_args() {
    let p = parse_ok(
        "fn main() -> int { v.push(1, 2).len(); return 0; }",
    );
    // f() -> { v.push(1, 2).len(); } as an expression statement
    let stmt = &p.funcs[0].body.0[0];
    let wlel::ast::StmtKind::ExprStmt(e) = &stmt.node else {
        panic!("expr stmt");
    };
    let ExprKind::MethodCall(recv, name, _) = &e.node else {
        panic!("outer method call");
    };
    assert_eq!(name, "len");
    let ExprKind::MethodCall(_, inner, args) = &recv.node else {
        panic!("inner method call");
    };
    assert_eq!(inner, "push");
    assert_eq!(args.len(), 2);
}

// ---------------------------------------------------------------------------
// checking + codegen

#[test]
fn methods_desugar_into_functions() {
    let c = front(
        "struct Pt { x: float, y: float }
         impl Pt {
             fn len(self) -> float { return self.x * self.x + self.y * self.y; }
             fn scale(self: *Pt, k: float) { self.x *= k; }
         }
         fn main() -> int {
             p := Pt { x: 1.0, y: 2.0 };
             p.scale(2.0);
             return p.len() as int;
         }",
    )
    .expect("typecheck");
    // the desugared functions exist with mangled names
    assert!(c.contains("double Pt__len(Pt self) {"), "{c}");
    assert!(c.contains("void Pt__scale(Pt* self, double k) {"), "{c}");
    // call sites are plain calls
    assert!(c.contains("Pt__scale(&p, 2.0);"), "{c}");
    assert!(c.contains("Pt__len(p)"), "{c}");
}

#[test]
fn value_method_on_pointer_receiver_auto_derefs() {
    let c = front(
        "struct Pt { x: float, y: float }
         impl Pt {
             fn len(self) -> float { return self.x; }
         }
         fn main() -> int {
             p := Pt { x: 1.0, y: 2.0 };
             pp := &p;
             return pp.len() as int;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("Pt__len((*pp))"), "{c}");
}

#[test]
fn generic_impl_monomorphizes_per_receiver() {
    let c = front(
        "use std;
         struct Box[T] { val: T }
         impl Box[T] {
             fn get(self) -> T { return self.val; }
             fn set(self: *Box[T], v: T) { self.val = v; }
         }
         fn main() -> int {
             b := Box { val: 1 };
             b.set(2);
             s := Box { val: \"x\" };
             return b.get() + std::strlen(s.get());
         }",
    )
    .expect("typecheck");
    assert!(c.contains("long long Box__get__int(Box__int self) {"), "{c}");
    assert!(c.contains("void Box__set__int(Box__int* self, long long v) {"), "{c}");
    assert!(c.contains("Box__get__string"), "{c}");
    // unrelated instantiations are not emitted
    assert!(!c.contains("Box__get__float"), "{c}");
}

#[test]
fn std_method_layer_end_to_end() {
    let (ok, out, err) = run_cli(
        "std_methods",
        r#"use std;
           fn main() -> int {
               v := vec_new[int]();
               v.push(10);
               v.push(32);
               std::println_int(v.len());
               std::println_int(v.pop());
               m := map_new[string, int]();
               m.set("k", 5);
               std::println_int(m.get("k"));
               let r: Result[int, string] = Ok(7);
               std::println_int(r.unwrap());
               let bad: Result[int, string] = Err("boom");
               if bad.is_err() { std::println_int(0); }
               o := Some(3);
               std::println_int(o.unwrap());
               return 0;
           }"#,
    );
    assert!(ok, "{err}");
    assert!(out.contains("2"), "{out}");
    assert!(out.contains("32"), "{out}");
    assert!(out.contains("5"), "{out}");
    assert!(out.contains("7"), "{out}");
    assert!(out.contains("3"), "{out}");
}

#[test]
fn methods_work_inside_arena_and_defer() {
    let (ok, out, err) = run_cli(
        "arena_methods",
        r#"use std;
           struct Buf { n: int }
           impl Buf {
               fn bump(self: *Buf) { self.n += 1; }
               fn get(self) -> int { return self.n; }
           }
           fn main() -> int {
               total := 0;
               arena(1 << 16) {
                   b := Buf { n: 0 };
                   defer { total = b.get(); }
                   b.bump();
                   b.bump();
               }
               std::println_int(total);
               return 0;
           }"#,
    );
    assert!(ok, "{err}");
    assert!(out.contains("2"), "{out}");
}

// ---------------------------------------------------------------------------
// error cases

#[test]
fn unknown_method_rejected() {
    let msg = check_err_msg(
        "struct Pt { x: float }
         impl Pt { fn len(self) -> float { return self.x; } }
         fn main() -> int { p := Pt { x: 1.0 }; p.nope(); return 0; }",
    );
    assert!(msg.contains("type 'Pt' has no method 'nope'"), "{msg}");
}

#[test]
fn methods_only_exist_on_impl_types() {
    let msg = check_err_msg(
        "struct A { x: int }
         impl A { fn f(self) -> int { return self.x; } }
         fn main() -> int { x := 5; x.f(); return 0; }",
    );
    assert!(msg.contains("type 'int' has no methods"), "{msg}");
}

#[test]
fn impl_on_unknown_type_rejected() {
    let msg = check_err_msg(
        "impl Ghost { fn f(self) -> int { return 0; } }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("impl on unknown type 'Ghost'"), "{msg}");
}

#[test]
fn duplicate_method_rejected() {
    let msg = check_err_msg(
        "struct Pt { x: int }
         impl Pt { fn f(self) -> int { return self.x; } }
         impl Pt { fn f(self) -> int { return self.x; } }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("duplicate method 'Pt.f'"), "{msg}");
}

#[test]
fn method_without_self_rejected() {
    let msg = check_err_msg(
        "struct Pt { x: int }
         impl Pt { fn f() -> int { return 0; } }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("method 'f' must take 'self' as its first parameter"), "{msg}");
}

#[test]
fn method_own_type_params_rejected() {
    let msg = check_err_msg(
        "struct Pt { x: int }
         impl Pt { fn f[T](self) -> int { return self.x; } }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("method 'f' cannot declare its own type parameters"), "{msg}");
}

#[test]
fn impl_params_must_repeat_the_type_params() {
    let msg = check_err_msg(
        "struct Box[T] { val: T }
         impl Box[U] { fn get(self) -> U { return self.val; } }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("impl parameters must use the type's own names"), "{msg}");
    let msg = check_err_msg(
        "struct Pt { x: int }
         impl Pt[T] { fn get(self) -> int { return self.x; } }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("impl of non-generic type 'Pt' cannot take type parameters"), "{msg}");
    let msg = check_err_msg(
        "struct Box[T] { val: T }
         impl Box { fn get(self) -> int { return self.val; } }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("impl Box[] must repeat the type's own parameters"), "{msg}");
}

#[test]
fn auto_borrow_needs_an_lvalue() {
    let msg = check_err_msg(
        "struct Cnt { n: int }
         impl Cnt { fn bump(self: *Cnt) { self.n += 1; } }
         fn make() -> Cnt { return Cnt { n: 0 }; }
         fn main() -> int { make().bump(); return 0; }",
    );
    assert!(msg.contains("takes self by pointer"), "{msg}");
}

#[test]
fn method_name_colliding_with_function_rejected() {
    let msg = check_err_msg(
        "struct Pt { x: int }
         fn Pt__f(v: int) -> int { return v; }
         impl Pt { fn f(self) -> int { return self.x; } }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("duplicate function 'Pt__f'"), "{msg}");
}

// ---------------------------------------------------------------------------
// fmt

#[test]
fn fmt_impl_blocks_are_canonical_and_idempotent() {
    let src = "use std;\nstruct Pt { x: float, y: float }\nimpl Pt {\nfn len(self) -> float {\nreturn self.x;\n}\nfn scale(self: *Pt, k: float) { self.x *= k; }\n}\nfn main() -> int { p := Pt { x: 1.0, y: 2.0 }; p.scale(2.0); return 0; }\n";
    let once = format_source(src).expect("formats");
    // canonical shape: impl head, indented methods, blank lines between them
    assert!(once.contains("impl Pt {"), "{once}");
    assert!(once.contains("    fn len(self) -> float {"), "{once}");
    assert!(once.contains("    fn scale(self: *Pt, k: float) {"), "{once}");
    let twice = format_source(&once).expect("refmt");
    assert_eq!(once, twice, "fmt is idempotent");
}

#[test]
fn fmt_keeps_method_calls_with_postfix_precedence() {
    let src = "fn main() -> int { v.push(1 + 2).len(); return -(1); }\n";
    let out = format_source(src).expect("formats");
    assert!(out.contains("v.push(1 + 2).len();"), "{out}");
    // -(1) is straightened to -1 (conservative unary rule)
    assert!(out.contains("return -1;"), "{out}");
}

// ---------------------------------------------------------------------------
// CLI

#[test]
fn cli_run_and_test_with_methods() {
    let (ok, out, err) = run_cli(
        "cli_methods",
        r#"use std;
           struct Pt { x: float, y: float }
           impl Pt {
               fn len(self) -> float { return self.x * self.x + self.y * self.y; }
           }
           fn main() -> int {
               p := Pt { x: 3.0, y: 4.0 };
               std::println_int(p.len() as int);
               return 0;
           }
           test "methods via cli" {
               p := Pt { x: 1.0, y: 1.0 };
               assert_eq(p.len(), 2.0);
           }"#,
    );
    assert!(ok, "{err}");
    assert!(out.contains("25"), "{out}");

    let dir = std::env::temp_dir().join(format!("wlel_impl_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("t.wl");
    fs::write(
        &path,
        r#"struct Pt { x: int }
           impl Pt { fn twice(self) -> int { return self.x * 2; } }
           test "twice" {
               p := Pt { x: 21 };
               assert_eq(p.twice(), 42);
           }"#,
    )
    .expect("write");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("1 passed"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn impls_flow_through_project_deps() {
    let dir =
        std::env::temp_dir().join(format!("wlel_impl_proj_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("lib/src")).expect("mkdir lib");
    fs::create_dir_all(dir.join("app/src")).expect("mkdir app");
    fs::write(
        dir.join("lib/wlel.toml"),
        "[package]\nname = \"geomlib\"\nversion = \"0.1.0\"\n",
    )
    .expect("write lib manifest");
    fs::write(
        dir.join("lib/src/lib.wl"),
        "use std;\nstruct Pt { x: float, y: float }\nimpl Pt {\n    fn len(self) -> float {\n        return std::math::sqrt(self.x * self.x + self.y * self.y);\n    }\n    fn scale(self: *Pt, k: float) {\n        self.x *= k;\n    }\n}\n",
    )
    .expect("write lib");
    fs::write(
        dir.join("app/wlel.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[deps]\ngeomlib = { path = \"../lib\" }\n",
    )
    .expect("write app manifest");
    fs::write(
        dir.join("app/src/main.wl"),
        "use std;\nfn main() -> int {\n    p := Pt { x: 3.0, y: 4.0 };\n    p.scale(2.0);\n    std::println_int(p.len() as int);\n    return 0;\n}\n",
    )
    .expect("write main");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .arg("run")
        .current_dir(dir.join("app"))
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("7"), "{out}"); // sqrt(52) = 7.21...
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unsafe_std_wl_is_canonical() {
    // the embedded library must parse+check on its own (it is spliced into
    // every `use std` program); a broken std is a build error
    let std_prog = wlel::stdsrc::parse_std();
    assert!(!std_prog.impls.is_empty(), "std carries impl blocks");
    let names: Vec<&str> = std_prog
        .impls
        .iter()
        .map(|i| i.type_name.as_str())
        .collect();
    assert!(names.contains(&"Vec"), "{names:?}");
    assert!(names.contains(&"HashMap"), "{names:?}");
    assert!(names.contains(&"Result"), "{names:?}");
    assert!(names.contains(&"Option"), "{names:?}");
}
