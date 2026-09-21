//! Generic functions and structs: monomorphization at call sites,
//! inference, mangled C output, and the error cases around inference.

use std::fs;

use std::process::Command;
use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::parser::Parser;

fn front(src: &str) -> Result<String, String> {
    let toks = Lexer::new(src).tokenize().map_err(|e| e.to_string())?;
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    for f in p.funcs.iter_mut() {
        f.file = "gen.wl".into();
    }
    Checker::check(&mut p).map_err(|e| e.msg)?;
    Ok(gen_program(&p))
}

fn check_err_msg(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    Checker::check(&mut p).expect_err("must be rejected").msg
}

#[test]
fn generic_fn_instantiates_per_type() {
    let c = front(
        "fn id[T](x: T) -> T { return x; }
         fn main() -> int {
             a := id(5);
             b := id(2.5);
             s := id(\"hi\");
             let u: u8 = id(7u8);
             return 0;
         }",
    )
    .expect("typecheck");
    // one concrete C function per distinct type argument
    assert!(c.contains("long long id__int(long long x)"), "{c}");
    assert!(c.contains("double id__float(double x)"), "{c}");
    assert!(c.contains("const char* id__string(const char* x)"), "{c}");
    assert!(c.contains("uint8_t id__u8(uint8_t x)"), "{c}");
    // call sites use the mangled names
    assert!(c.contains("id__int(5)"), "{c}");
    assert!(c.contains("id__float(2.5)"), "{c}");
}

#[test]
fn generic_fn_used_zero_times_is_not_emitted() {
    let c = front(
        "fn unused_gen[T](x: T) -> T { return x; }
         fn main() -> int { return 0; }",
    )
    .expect("typecheck");
    assert!(!c.contains("unused_gen"), "monomorphization is lazy: {c}");
}

#[test]
fn two_type_params_infer_independently() {
    let c = front(
        "fn pair[K, V](k: K, v: V) -> K { return k; }
         fn main() -> int {
             pair(1, \"x\");
             pair(\"y\", 2.5);
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("pair__int__string(1, \"x\")"), "{c}");
    assert!(c.contains("pair__string__float(\"y\", 2.5)"), "{c}");
}

#[test]
fn conflicting_bindings_rejected() {
    let msg = check_err_msg(
        "fn first[T](a: T, b: T) -> T { return a; }
         fn main() -> int { return first(1, 2.5) as int; }",
    );
    assert!(msg.contains("cannot infer type parameter 'T'"), "{msg}");
    assert!(msg.contains("int vs float"), "{msg}");
}

#[test]
fn generic_bodies_are_checked_lazily() {
    // a generic that is never called is never monomorphized, so its body —
    // even a broken one — is never checked and nothing reaches the C output
    let c = front(
        "fn dead[T](x: int) -> T { return 0 as T; }
         fn main() -> int { return 0; }",
    )
    .expect("uncalled generics cost nothing");
    assert!(!c.contains("dead"), "{c}");
}

#[test]
fn return_only_type_param_rejected() {
    let msg = check_err_msg(
        "fn wrap[T](n: int) -> T { return 0 as T; }
         fn main() -> int { return wrap(1) as int; }",
    );
    assert!(msg.contains("does not appear in any parameter type"), "{msg}");
}

#[test]
fn generic_struct_instantiates_and_carries_pointer_fields() {
    let c = front(
        "struct Node[T] {
             val: T,
             next: *Node[T],
         }
         fn main() -> int {
             arena(1024) {
                 n := new(Node[int]);
                 n.val = 9;
                 n.next = 0 as *Node[int];
                 return n.val;
             }
             return 0;
         }",
    )
    .expect("typecheck");
    // self-referential field points at the mangled struct
    assert!(c.contains("struct Node__int {"), "{c}");
    assert!(c.contains("Node__int* next;"), "{c}");
    assert!(c.contains("sizeof(Node__int)"), "{c}");
}

#[test]
fn generic_struct_field_string_type() {
    let c = front(
        "struct Box[T] { val: T }
         fn main() -> int {
             b := Box { val: \"halo\" };
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("struct Box__string {"), "{c}");
    assert!(c.contains("const char* val;"), "{c}");
    assert!(c.contains("(Box__string){ .val = \"halo\" }"), "{c}");
}

#[test]
fn nested_generic_args() {
    let c = front(
        "struct Box[T] { val: T }
         fn main() -> int {
             b := Box { val: Box { val: 3 } };
             return b.val.val;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("struct Box__Box__int {"), "{c}");
    assert!(c.contains("Box__Box__int {"), "{c}");
    assert!(c.contains("Box__Box__int b = (Box__Box__int){ .val = (Box__int){ .val = 3 } };"), "{c}");
}

#[test]
fn wrong_generic_arg_count_rejected() {
    let msg = check_err_msg(
        "struct Pair[K, V] { k: K, v: V }
         fn main() -> int {
             let p: Pair[int] = Pair { k: 1, v: 2 };
             return 0;
         }",
    );
    assert!(msg.contains("takes 2 type argument(s), got 1"), "{msg}");
}

#[test]
fn type_param_shadowing_builtin_rejected() {
    let msg = check_err_msg("fn id[int](x: int) -> int { return x; } fn main() -> int { return 0; }");
    assert!(msg.contains("shadows a built-in type"), "{msg}");
}

#[test]
fn generic_and_concrete_fn_name_collision_rejected() {
    let msg = check_err_msg(
        "fn f[T](x: T) -> T { return x; }
         fn f(x: int) -> int { return x; }
         fn main() -> int { return 0; }",
    );
    assert!(msg.contains("duplicate function 'f'"), "{msg}");
}

#[test]
fn generic_fn_params_must_be_annotated() {
    let msg = check_err_msg("fn id[T](x) -> T { return x; } fn main() -> int { return 0; }");
    assert!(msg.contains("needs a type annotation"), "{msg}");
}

#[test]
fn recursion_within_one_instantiation_terminates() {
    let c = front(
        "fn fact[T](n: T, acc: T) -> T {
             if n <= 1 { return acc; }
             return fact(n - 1, acc * n);
         }
         fn main() -> int {
             return fact(5, 1);
         }",
    )
    .expect("typecheck");
    // exactly ONE concrete function, self-recursive
    let count = c.matches("fact__int(").count();
    assert!(count >= 2, "definition + recursive calls: {c}");
    assert!(!c.contains("fact__float"), "{c}");
}

#[test]
fn imported_generic_struct_field_in_concrete_struct() {
    let c = front(
        "struct Box[T] { val: T }
         struct Holder {
             b: Box[int],
         }
         fn main() -> int {
             h := Holder { b: Box { val: 5 } };
             return h.b.val;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("struct Holder {"), "{c}");
    assert!(c.contains("Box__int b;"), "{c}");
}

// ---------------------------------------------------------------------------
// end-to-end: the binary actually runs and the tests-as-generics path works

#[test]
fn generics_end_to_end_run_and_test() {
    let dir = std::env::temp_dir().join(format!("wlel_gen_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("gen.wl");
    fs::write(
        &path,
        r#"use std;
           struct Box[T] { val: T }
           fn box_get[T](b: Box[T]) -> T { return b.val; }
           fn main() -> int {
               std::println_int(box_get(Box { val: 21 }) * 2);
               return 0;
           }
           test "generic" {
               assert_eq(box_get(Box { val: 5 }), 5);
           }"#,
    )
    .expect("write gen.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(String::from_utf8_lossy(&run.stdout).contains("42"));

    let test = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(test.status.success(), "{}", String::from_utf8_lossy(&test.stderr));
    assert!(String::from_utf8_lossy(&test.stdout).contains("1 passed"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn generic_fns_compose_with_safe_mode_and_arena() {
    let dir = std::env::temp_dir().join(format!("wlel_gen2_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("gen_arena.wl");
    fs::write(
        &path,
        r#"use std;
           struct Pair[K, V] { k: K, v: V }
           fn swap[K](a: *K, b: *K) {
               t := *a;
               *a = *b;
               *b = t;
           }
           fn main() -> int {
               arena(1024) {
                   p := new(Pair[int, string]);
                   p.k = 1;
                   p.v = "satu";
                   k := 10;
                   v := 20;
                   swap(&k, &v);
                   std::println_int(k + v + p.k);
                   std::println_str(p.v);
               }
               return 0;
           }"#,
    )
    .expect("write gen_arena.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("31"), "{out}");
    assert!(out.contains("satu"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}
