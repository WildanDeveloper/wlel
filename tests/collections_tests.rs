//! Standard-library collections (Vec[T], HashMap[K, V]) written in Wlel and
//! injected by `use std`: monomorphization, growth/rehash behavior,
//! for-in iteration, and the error cases around keys and iteration.

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
            f.file = "col.wl".into();
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
            f.file = "col.wl".into();
        }
    }
    Checker::check(&mut p).expect_err("must be rejected").msg
}

// ---------------------------------------------------------------------------
// Vec

#[test]
fn vec_push_get_monomorphizes_per_type() {
    let c = front(
        "fn main() -> int {
             v := vec_new[int]();
             vec_push(&v, 7);
             sv := vec_new[string]();
             vec_push(&sv, \"x\");
             hit := 0;
             if vec_get(&sv, 0) == \"x\" { hit = 1; }
             return vec_get(&v, 0) + hit;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("vec_new__int()"), "{c}");
    assert!(c.contains("vec_push__int(&v, 7)"), "{c}");
    assert!(c.contains("struct Vec__int {"), "{c}");
    assert!(c.contains("struct Vec__string {"), "{c}");
}

#[test]
fn vec_growth_beyond_initial_capacity_end_to_end() {
    let dir = std::env::temp_dir().join(format!("wlel_vec_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("vec.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               v := vec_new[int]();
               total := 0;
               for i in 0..200 {
                   vec_push(&v, i * 2);
               }
               for x in v {
                   total += x;
               }
               // 0+2+...+398 = 199*200
               std::println_int(total);
               std::println_int(vec_len(&v));
               vec_set(&v, 0, 100);
               std::println_int(vec_get(&v, 0));
               p := vec_pop(&v);
               std::println_int(p);
               std::println_int(vec_len(&v));
               return 0;
           }"#,
    )
    .expect("write vec.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("39800"), "{out}");
    assert!(out.contains("200"), "{out}");
    assert!(out.contains("398"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn vec_of_structs_and_nested_vecs() {
    let c = front(
        "struct Pt { x: float, y: float }
         fn main() -> int {
             pts := vec_new[Pt]();
             vec_push(&pts, Pt { x: 1.0, y: 2.0 });
             outer := vec_new[Vec[int]]();
             inner := vec_new[int]();
             vec_push(&inner, 5);
             vec_push(&outer, inner);
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("struct Vec__Pt {"), "{c}");
    assert!(c.contains("struct Vec__Vec__int {"), "{c}");
}

#[test]
fn vec_string_elements_roundtrip() {
    let dir = std::env::temp_dir().join(format!("wlel_vs_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("vs.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               names := vec_new[string]();
               vec_push(&names, "alice");
               vec_push(&names, "bob");
               found := 0;
               for n in names {
                   if n == "bob" {
                       found += 1;
                   }
               }
               std::println_int(found);
               first := vec_get(&names, 0);
               std::println_str("hi " + first);
               return 0;
           }"#,
    )
    .expect("write vs.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("hi alice"), "{out}");
    assert!(out.starts_with("1\n"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn vec_in_arena_is_lsan_clean() {
    let dir = std::env::temp_dir().join(format!("wlel_va_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("va.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               arena(1 << 16) {
                   v := vec_new[int]();
                   for i in 0..1000 {
                       vec_push(&v, i);
                   }
                   s := 0;
                   for x in v { s += x; }
                   std::println_int(s);
               }
               return 0;
           }"#,
    )
    .expect("write va.wl");
    let bin = dir.join("va");
    let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", path.to_str().unwrap(), "-o", bin.to_str().unwrap(), "-sanitize"])
        .output()
        .expect("spawn wlel");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&bin).output().expect("run sanitized binary");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(String::from_utf8_lossy(&run.stdout).contains("499500"), "{}", String::from_utf8_lossy(&run.stdout));
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// for-in

#[test]
fn for_in_over_array_and_vec_snapshot() {
    let dir = std::env::temp_dir().join(format!("wlel_forin_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("forin.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               // arrays iterate in place
               arr := [10, 20, 30];
               sum := 0;
               for x in arr { sum += x; }
               std::println_int(sum); // 60

               // Vec iteration is a snapshot: pushing during the loop does
               // not extend it, and the loop sees the pre-push contents
               v := vec_new[int]();
               vec_push(&v, 1);
               vec_push(&v, 2);
               seen := 0;
               for x in v {
                   seen += 1;
                   vec_push(&v, x * 10);
               }
               std::println_int(seen); // 2, not 4
               std::println_int(vec_len(&v)); // 4 after the loop
               return 0;
           }"#,
    )
    .expect("write forin.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("60"), "{out}");
    assert!(out.contains('2'), "{out}");
    assert!(out.contains('4'), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn for_in_emits_bounds_checks_in_safe_mode_only() {
    let src = "fn main() -> int {
                   v := vec_new[int]();
                   vec_push(&v, 3);
                   t := 0;
                   for x in v { t += x; }
                   return t;
               }";
    let toks = Lexer::new(src).tokenize().unwrap();
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "{errs:?}");
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
            f.file = "col.wl".into();
        }
    }
    Checker::check(&mut p).expect("typecheck");
    let safe = wlel::codegen::gen_program_safe(&p);
    let release = gen_program(&p);
    assert!(safe.contains("_wlel_arr_at"), "dev mode bounds-checks for-in: {safe}");
    assert!(!release.contains("_wlel_arr_at"), "release has zero checks: {release}");
}

#[test]
fn for_in_over_non_iterable_rejected() {
    let msg = check_err_msg(
        "fn main() -> int {
             x := 5;
             for y in x { return 0; }
             return 0;
         }",
    );
    assert!(msg.contains("cannot iterate over int"), "{msg}");
}

#[test]
fn for_in_loop_var_is_scoped_and_typed() {
    let c = front(
        "fn main() -> int {
             arr := [1, 2];
             for x in arr {
                 x += 1;
             }
             return 0;
         }",
    )
    .expect("typecheck");
    // the loop variable is a proper long long copy, mutated freely
    assert!(c.contains("long long x = _wlel_it"), "{c}");
}

// ---------------------------------------------------------------------------
// HashMap

#[test]
fn map_set_get_overwrite_delete() {
    let dir = std::env::temp_dir().join(format!("wlel_map_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("map.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               m := map_new[string, int]();
               c0 := 0;
               if map_has(&m, "a") { c0 = 1; }
               std::println_int(c0);
               map_set(&m, "a", 1);
               map_set(&m, "b", 2);
               map_set(&m, "a", 10); // overwrite
               std::println_int(map_get(&m, "a"));
               std::println_int(m.len);
               d1 := 0;
               if map_del(&m, "a") { d1 = 1; }
               std::println_int(d1);
               d2 := 0;
               if map_del(&m, "zz") { d2 = 1; }
               std::println_int(d2);
               std::println_int(m.len);
               std::println_int(map_get_or(&m, "a", -1));
               return 0;
           }"#,
    )
    .expect("write map.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    for expect in ["0", "10", "2", "1", "0", "1", "-1"] {
        assert!(out.contains(expect), "want {expect} in:\n{out}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn map_rehashes_across_many_inserts() {
    let dir = std::env::temp_dir().join(format!("wlel_rehash_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("rehash.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               m := map_new[int, string]();
               for i in 0..300 {
                   map_set(&m, i, "val");
               }
               ok := 0;
               for i in 0..300 {
                   if map_get(&m, i) == "val" {
                       ok += 1;
                   }
               }
               std::println_int(ok);
               // delete a third, re-check the rest survive tombstones
               for i in 0..100 {
                   map_del(&m, i);
               }
               std::println_int(m.len);
               h1 := 0;
               if map_has(&m, 299) { h1 = 1; }
               std::println_int(h1);
               h2 := 0;
               if map_has(&m, 5) { h2 = 1; }
               std::println_int(h2);
               return 0;
           }"#,
    )
    .expect("write rehash.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("300"), "{out}");
    assert!(out.contains("200"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn map_keys_enables_iteration() {
    let dir = std::env::temp_dir().join(format!("wlel_mk_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("mk.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               m := map_new[string, int]();
               map_set(&m, "x", 5);
               map_set(&m, "y", 7);
               total := 0;
               n := 0;
               for k in map_keys(&m) {
                   total += map_get(&m, k);
                   n += 1;
               }
               std::println_int(total);
               std::println_int(n);
               return 0;
           }"#,
    )
    .expect("write mk.wl");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("12"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn map_with_struct_values() {
    let c = front(
        "struct Cfg { w: int, h: int }
         fn main() -> int {
             m := map_new[string, Cfg]();
             map_set(&m, \"win\", Cfg { w: 80, h: 25 });
             got := map_get(&m, \"win\");
             return got.w + got.h;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("struct HashMap__string__Cfg {"), "{c}");
}

#[test]
fn map_int_and_float_and_bool_keys() {
    let c = front(
        "fn main() -> int {
             mi := map_new[int, int]();
             map_set(&mi, 1, 10);
             mf := map_new[float, string]();
             map_set(&mf, 1.5, \"one\");
             mb := map_new[bool, int]();
             map_set(&mb, true, 1);
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("map_set__int__int(&mi, 1, 10)"), "{c}");
    assert!(c.contains("_wlel_hash_f64"), "{c}");
    assert!(c.contains("_wlel_hash_i64"), "{c}");
}

#[test]
fn struct_keys_are_rejected_with_clear_error() {
    let msg = check_err_msg(
        "struct K { a: int }
         fn main() -> int {
             m := map_new[K, int]();
             map_set(&m, K { a: 1 }, 2);
             return 0;
         }",
    );
    // the generic body rejects struct keys at instantiation time (hashing
    // has no implementation for them — the eq check backs it up)
    assert!(msg.contains("hash key must be"), "{msg}");
}

// ---------------------------------------------------------------------------
// explicit type arguments

#[test]
fn explicit_type_args_land_in_mangled_calls() {
    let c = front(
        "fn id[T](x: T) -> T { return x; }
         fn main() -> int {
             a := id[int](5);
             s := id[string](\"z\");
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("id__int(5)"), "{c}");
    assert!(c.contains("id__string(\"z\")"), "{c}");
}

#[test]
fn explicit_type_arg_count_mismatch_rejected() {
    let msg = check_err_msg(
        "fn id[T](x: T) -> T { return x; }
         fn main() -> int { return id[int, string](5); }",
    );
    assert!(msg.contains("takes 1 type argument(s), got 2"), "{msg}");
}

#[test]
fn type_args_on_non_generic_fn_rejected() {
    let msg = check_err_msg(
        "fn f(x: int) -> int { return x; }
         fn main() -> int { return f[int](1); }",
    );
    assert!(msg.contains("is not generic"), "{msg}");
}

// ---------------------------------------------------------------------------
// regression: concrete pointer-typed fields in generic struct literals
// (HashMap.state: *u8 hit a unify_pattern bug — keep it covered)

#[test]
fn generic_struct_literal_with_concrete_pointer_field() {
    let c = front(
        "struct Holder[T] {
             tag: *u8,
             val: T,
         }
         fn main() -> int {
             h := Holder { tag: 0 as *u8, val: 9 };
             return h.val;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("struct Holder__int {"), "{c}");
    assert!(c.contains("uint8_t* tag;"), "{c}");
}

// ---------------------------------------------------------------------------
// lazy emission + test harness integration

#[test]
fn uncalled_std_functions_are_never_emitted() {
    let c = front(
        "fn main() -> int {
             v := vec_new[int]();
             vec_push(&v, 1);
             return vec_get(&v, 0);
         }",
    )
    .expect("typecheck");
    assert!(c.contains("vec_push__int"), "{c}");
    // never instantiated: generic templates cost nothing unless called
    assert!(!c.contains("map_set__"), "{c}");
    assert!(!c.contains("vec_pop__"), "{c}");
}

#[test]
fn collections_work_in_wlel_test_harness() {
    let dir = std::env::temp_dir().join(format!("wlel_ct_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("ct.wl");
    fs::write(
        &path,
        r#"use std;
           test "vec sums" {
               v := vec_new[int]();
               vec_push(&v, 2);
               vec_push(&v, 3);
               s := 0;
               for x in v { s += x; }
               assert_eq(s, 5);
           }
           test "map roundtrip" {
               m := map_new[int, string]();
               map_set(&m, 42, "jawaban");
               assert_eq(map_get(&m, 42), "jawaban");
               assert(map_has(&m, 42));
               assert(!map_has(&m, 41));
           }"#,
    )
    .expect("write ct.wl");
    let test = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(test.status.success(), "{}", String::from_utf8_lossy(&test.stderr));
    let out = String::from_utf8_lossy(&test.stdout);
    assert!(out.contains("2 passed"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}
