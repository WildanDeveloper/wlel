//! Standard string library (Fase 2): str_find/str_sub/str_trim/str_split/
//! str_parse_int/int_to_str written in Wlel and injected by `use std`, the
//! std::format/parse_float/float_to_str builtins, and read-only string
//! indexing (`s[i]` -> u8).

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
            f.file = "str.wl".into();
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
            f.file = "str.wl".into();
        }
    }
    Checker::check(&mut p).expect_err("must be rejected").msg
}

fn run_cli(name: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_str_{name}_{}", std::process::id()));
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

// ---------------------------------------------------------------------------
// Wlel-written functions: monomorphization + emitted C

#[test]
fn str_funcs_are_std_functions_not_builtins() {
    let c = front(
        "fn main() -> int {
             v := str_split(\"a,b\", \",\");
             return vec_len(&v) + str_find(\"ab\", \"b\");
         }",
    )
    .expect("typecheck");
    // written in Wlel: they flow through normal monomorphization
    assert!(c.contains("vec_push__string"), "{c}");
    assert!(c.contains("static long long str_find(const char* hay, const char* needle)"), "{c}");
    assert!(c.contains("static long long str_find_from(const char* hay, const char* needle, long long from)"), "{c}");
}

#[test]
fn int_to_str_emits_byte_buffer_code() {
    let c = front(
        "use std;
         fn main() -> int {
             std::println_str(int_to_str(42));
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("static const char* int_to_str(long long v)"), "{c}");
    // builds the digits through a u8 buffer, not snprintf
    assert!(c.contains("uint8_t* buf = (uint8_t*)"), "{c}");
}

// ---------------------------------------------------------------------------
// std::format: rewrite, wrappers, zero bloat

#[test]
fn format_rewrites_to_per_signature_helper() {
    let c = front(
        "use std;
         fn main() -> int {
             std::println_str(std::format(\"{} {} {}\", 1, true, \"s\"));
             std::println_str(std::format(\"{}\", 2.5));
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("_wlel_format_ibs("), "{c}");
    assert!(c.contains("_wlel_format_f("), "{c}");
    assert!(c.contains("snprintf(b0, sizeof b0, \"%lld\", a0)"), "{c}");
    assert!(c.contains("parts[1] = a1 ? \"true\" : \"false\";"), "{c}");
    assert!(c.contains("static char* _wlel_format_join(const char* fmt, const char** parts, long long nparts)"), "{c}");
}

#[test]
fn format_helper_absent_when_unused() {
    let c = front(
        "use std;
         fn main() -> int {
             std::println_str(\"plain\");
             return 0;
         }",
    )
    .expect("typecheck");
    // the fixed _wlel_format_join prelude helper is always present (like
    // std__strlen); per-signature wrappers appear only at real call sites
    let count = c.matches("_wlel_format_").count();
    assert_eq!(count, 1, "only the join definition may mention format: {c}");
}

#[test]
fn format_rejects_unsupported_argument_types() {
    let msg = check_err_msg(
        "use std;
         struct P { x: int }
         fn main() -> int {
             std::println_str(std::format(\"{}\", P { x: 1 }));
             return 0;
         }",
    );
    assert!(msg.contains("std::format supports int, float, bool and string"), "{msg}");
}

#[test]
fn format_requires_format_string_first() {
    let msg = check_err_msg(
        "use std;
         fn main() -> int {
             std::println_str(std::format(1, 2));
             return 0;
         }",
    );
    assert!(msg.contains("format must be string"), "{msg}");
}

#[test]
fn format_without_arguments_is_allowed() {
    let c = front(
        "use std;
         fn main() -> int {
             std::println_str(std::format(\"nothing to say\"));
             return 0;
         }",
    )
    .expect("typecheck");
    assert!(c.contains("_wlel_format_0(\"nothing to say\")"), "{c}");
    assert!(c.contains("return _wlel_format_join(fmt, 0, 0);"), "{c}");
}

// ---------------------------------------------------------------------------
// string indexing: s[i] reads a byte, strings are immutable

#[test]
fn string_indexing_types_as_u8() {
    let c = front(
        "fn main() -> int {
             s := \"abc\";
             b := s[1];
             return b as int;
         }",
    )
    .expect("typecheck");
    // the checker annotates u8, codegen emits a plain C index
    assert!(c.contains("uint8_t b = s[1];"), "{c}");
}

#[test]
fn string_index_assignment_rejected() {
    let msg = check_err_msg(
        "fn main() -> int {
             s := \"abc\";
             s[0] = 120;
             return 0;
         }",
    );
    assert!(msg.contains("strings are immutable"), "{msg}");
}

#[test]
fn string_index_compound_assignment_rejected() {
    let msg = check_err_msg(
        "fn main() -> int {
             s := \"abc\";
             s[0] += 1;
             return 0;
         }",
    );
    assert!(msg.contains("strings are immutable"), "{msg}");
}

#[test]
fn indexing_non_string_still_rejected() {
    let msg = check_err_msg(
        "fn main() -> int {
             x := 5;
             return x[0] as int;
         }",
    );
    assert!(msg.contains("cannot index into int"), "{msg}");
}

// ---------------------------------------------------------------------------
// `use std` stays used when only bare std functions are called

#[test]
fn bare_std_calls_mark_import_used() {
    let dir = std::env::temp_dir().join(format!("wlel_struse_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("use.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               v := str_split("a,b", ",");
               std::println_int(vec_len(&v));
               return 0;
           }"#,
    )
    .expect("write");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(!err.contains("never used"), "bare std calls must silence the unused-import warning: {err}");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// end-to-end behavior via the CLI

#[test]
fn string_ops_end_to_end() {
    let (ok, out, err) = run_cli(
        "ops",
        r#"use std;
           fn main() -> int {
               std::println_int(str_find("hello world", "world"));
               std::println_int(str_find("hello", "xyz"));
               std::println_str(str_sub("hello world", 6, 5));
               std::println_str("<" + str_trim("  pad  ") + ">");
               parts := str_split("a,b,,c", ",");
               std::println_int(vec_len(&parts));
               for p in parts {
                   std::println_str("[" + p + "]");
               }
               n := 0;
               if str_parse_int("-42", &n) { std::println_int(n); }
               if str_parse_int("12x", &n) { std::println_int(1); } else { std::println_int(0); }
               std::println_str(int_to_str(-987654321));
               f := 0.0;
               if std::parse_float("2.5", &f) { std::println_float(f); }
               if std::parse_float("1.5x", &f) { std::println_int(1); } else { std::println_int(0); }
               std::println_str(std::float_to_str(1.75));
               std::println_str(std::format("{} {}", 1, "x"));
               return 0;
           }"#,
    );
    assert!(ok, "{err}");
    for expect in [
        "6\n", "-1\n", "world\n", "<pad>\n", "4\n", "[a]\n", "[b]\n", "[]\n", "[c]\n",
        "-42\n", "0\n", "-987654321\n", "2.5\n", "0\n", "1.75\n", "1 x\n",
    ] {
        assert!(out.contains(expect), "want {expect:?} in:\n{out}");
    }
}

#[test]
fn i64_extremes_round_trip() {
    let (ok, out, err) = run_cli(
        "extremes",
        r#"use std;
           fn main() -> int {
               n := 0;
               assert(str_parse_int("9223372036854775807", &n));
               assert_eq(n, 9223372036854775807);
               assert(str_parse_int("-9223372036854775808", &n));
               assert_eq(n, -9223372036854775807 - 1);
               assert(!str_parse_int("9223372036854775808", &n));
               assert_eq(int_to_str(-9223372036854775807 - 1), "-9223372036854775808");
               std::println_str("ok");
               return 0;
           }"#,
    );
    assert!(ok, "{err}");
    assert!(out.contains("ok"), "{out}");
}

#[test]
fn split_with_empty_separator_aborts() {
    let (ok, _out, err) = run_cli(
        "emptysep",
        r#"use std;
           fn main() -> int {
               parts := str_split("abc", "");
               std::println_int(vec_len(&parts));
               return 0;
           }"#,
    );
    assert!(!ok, "empty separator must abort");
    assert!(err.contains("assertion failed"), "{err}");
}

#[test]
fn string_works_in_test_harness() {
    let dir = std::env::temp_dir().join(format!("wlel_strh_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("h.wl");
    fs::write(
        &path,
        r#"use std;
           test "format parse roundtrip" {
               n := 0;
               assert(str_parse_int(std::format("{}", 1234), &n));
               assert_eq(n, 1234);
           }
           test "split trim" {
               parts := str_split(str_trim("  x,y  "), ",");
               assert_eq(vec_len(&parts), 2);
               assert_eq(vec_get(&parts, 0), "x");
           }"#,
    )
    .expect("write");
    let test = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", path.to_str().unwrap()])
        .output()
        .expect("spawn wlel");
    assert!(test.status.success(), "{}", String::from_utf8_lossy(&test.stderr));
    let out = String::from_utf8_lossy(&test.stdout);
    assert!(out.contains("2 passed"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// memory safety

#[cfg(unix)]
#[test]
fn string_arena_program_is_asan_clean() {
    let dir = std::env::temp_dir().join(format!("wlel_strasan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("asan.wl");
    fs::write(
        &path,
        r#"use std;
           fn main() -> int {
               arena(1 << 16) {
                   joined := "";
                   for i in 0..200 {
                       for w in str_split("alpha beta gamma", " ") {
                           joined = std::format("{}[{}:{}]", joined, w, i);
                       }
                   }
                   n := 0;
                   assert(str_parse_int(str_sub(joined, 0, 0), &n) || true);
                   if std::strlen(joined) > 0 { std::println_int(1); } else { std::println_int(0); }
               }
               return 0;
           }"#,
    )
    .expect("write");
    let bin = dir.join("asan");
    let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["build", path.to_str().unwrap(), "-o", bin.to_str().unwrap(), "-sanitize"])
        .output()
        .expect("spawn wlel");
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));
    let run = Command::new(&bin).output().expect("run sanitized binary");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert!(String::from_utf8_lossy(&run.stdout).contains("1"), "{}", String::from_utf8_lossy(&run.stdout));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn release_build_has_no_format_or_check_overhead() {
    let src = "use std;
               fn main() -> int {
                   v := str_split(\"a,b,c\", \",\");
                   arr := [1, 2, 3];
                   std::println_str(std::format(\"{} {} {}\", vec_len(&v), arr[0], \"parts\"));
                   return 0;
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
            f.file = "str.wl".into();
        }
    }
    Checker::check(&mut p).expect("typecheck");
    let release = gen_program(&p);
    let safe = wlel::codegen::gen_program_safe(&p);
    // format wrappers exist in both; no dev-mode checks in release
    assert!(release.contains("_wlel_format_iis("), "{release}");
    assert!(!release.contains("_wlel_arr_at"), "release must be check-free: {release}");
    assert!(!release.contains("_wlel_divz"), "{release}");
    assert!(safe.contains("_wlel_arr_at"), "dev mode keeps bounds checks: {safe}");
}
