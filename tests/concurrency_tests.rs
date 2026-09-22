//! Concurrency v1 (Fase 3 item 4): `sys::thread/join`, `sys::mutex_*`,
//! `sys::chan_new[T]/send/recv/close/free`, `sys::sleep_ms` — name-resolved
//! workers with per-thread arenas, an unbounded typed FIFO channel, and the
//! honest C-like memory discipline (join frees the handle, mutex_free /
//! chan_free are explicit; the sanitizer mode catches the fallout).

use std::fs;
use std::process::Command;

use wlel::checker::Checker;
use wlel::codegen::{gen_program, gen_program_safe};
use wlel::lexer::Lexer;
use wlel::parser::Parser;
use wlel::stdsrc;

/// single-file front-end: parse, check, transpile
fn front(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    if p.uses.iter().any(|u| u.path.is_none()) {
        stdsrc::splice_std(&mut p);
    }
    Checker::check(&mut p).expect("typecheck");
    gen_program(&p)
}

fn front_safe(src: &str) -> String {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    if p.uses.iter().any(|u| u.path.is_none()) {
        stdsrc::splice_std(&mut p);
    }
    Checker::check(&mut p).expect("typecheck");
    gen_program_safe(&p)
}

fn check(src: &str) -> Result<(), String> {
    let toks = Lexer::new(src).tokenize().expect("lex ok");
    let (mut p, errs) = Parser::new(&toks).program();
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    if p.uses.iter().any(|u| u.path.is_none()) {
        stdsrc::splice_std(&mut p);
    }
    Checker::check(&mut p).map(|_| ()).map_err(|e| e.msg)
}

fn check_err(src: &str) -> String {
    check(src).unwrap_err()
}

// ---------------------------------------------------------------------------
// checking: threads

#[test]
fn thread_typechecks_and_yields_handle() {
    check(
        "fn work(x: int) { sys::sleep_ms(1); }
         fn main() {
             t := sys::thread(work, 5);
             sys::join(t);
         }",
    )
    .expect("basic spawn/join accepted");
}

#[test]
fn thread_needs_no_std_import() {
    check(
        "fn work(x: int) { }
         fn main() {
             sys::join(sys::thread(work, 1));
         }",
    )
    .expect("sys:: is always available");
}

#[test]
fn thread_worker_must_be_a_name() {
    let msg = check_err(
        "fn work(x: int) { }
         fn main() { t := sys::thread(work + 1, 5); }",
    );
    assert!(msg.contains("must be a function name"), "{msg}");
}

#[test]
fn thread_unknown_worker_rejected() {
    let msg = check_err("fn main() { t := sys::thread(nope, 5); }");
    assert!(msg.contains("unknown function 'nope'"), "{msg}");
}

#[test]
fn thread_generic_worker_rejected() {
    let msg = check_err(
        "fn gen[T](x: T) { }
         fn main() { t := sys::thread(gen, 5); }",
    );
    assert!(msg.contains("is generic"), "{msg}");
}

#[test]
fn thread_worker_must_return_void() {
    let msg = check_err(
        "fn calc(x: int) -> int { return x; }
         fn main() { t := sys::thread(calc, 5); }",
    );
    assert!(msg.contains("must return void"), "{msg}");
}

#[test]
fn thread_worker_must_take_exactly_one_parameter() {
    let msg = check_err(
        "fn no_args() { }
         fn main() { t := sys::thread(no_args, 5); }",
    );
    assert!(msg.contains("exactly 1 parameter"), "{msg}");
    let msg2 = check_err(
        "fn two(a: int, b: int) { }
         fn main() { t := sys::thread(two, 5); }",
    );
    assert!(msg2.contains("exactly 1 parameter"), "{msg2}");
}

#[test]
fn thread_data_must_match_worker_parameter() {
    let msg = check_err(
        "fn work(x: string) { }
         fn main() { t := sys::thread(work, 5); }",
    );
    assert!(msg.contains("data must be string, got int"), "{msg}");
    check(
        "struct Job { id: int }
         fn work(j: *Job) { }
         fn main() {
             j := Job { id: 1 };
             sys::join(sys::thread(work, &j));
         }",
    )
    .expect("pointer payload accepted");
}

// ---------------------------------------------------------------------------
// checking: mutexes

#[test]
fn mutex_ops_typecheck() {
    check(
        "fn main() {
             m := sys::mutex_new();
             sys::mutex_lock(m);
             sys::mutex_unlock(m);
             sys::mutex_free(m);
         }",
    )
    .expect("mutex lifecycle accepted");
}

#[test]
fn mutex_ops_reject_non_mutex() {
    let msg = check_err("fn main() { sys::mutex_lock(5); }");
    assert!(msg.contains("m must be *Mutex, got int"), "{msg}");
    let msg2 = check_err("fn main() { sys::mutex_new(1); }");
    assert!(msg2.contains("takes no arguments"), "{msg2}");
}

// ---------------------------------------------------------------------------
// checking: channels

#[test]
fn chan_lifecycle_typechecks() {
    check(
        "fn main() {
             ch := sys::chan_new[int]();
             assert(sys::chan_send(ch, 5));
             x := 0;
             assert(sys::chan_recv(ch, &x));
             sys::chan_close(ch);
             sys::chan_free(ch);
         }",
    )
    .expect("chan lifecycle accepted");
}

#[test]
fn chan_new_requires_exactly_one_type_argument() {
    let msg = check_err("fn main() { ch := sys::chan_new(); }");
    assert!(msg.contains("exactly 1 type argument"), "{msg}");
    let msg2 = check_err("fn main() { ch := sys::chan_new[int, int](); }");
    assert!(msg2.contains("exactly 1 type argument"), "{msg2}");
}

#[test]
fn chan_elem_void_and_array_rejected() {
    let msg = check_err("fn main() { ch := sys::chan_new[void](); }");
    assert!(msg.contains("element type cannot be void"), "{msg}");
    let msg2 = check_err("fn main() { ch := sys::chan_new[[int; 3]](); }");
    assert!(msg2.contains("wrap the array in a struct"), "{msg2}");
}

#[test]
fn chan_send_value_must_match_element() {
    let msg = check_err(
        "fn main() {
             ch := sys::chan_new[int]();
             sys::chan_send(ch, \"nope\");
         }",
    );
    assert!(msg.contains("value must be int, got string"), "{msg}");
}

#[test]
fn chan_recv_out_must_be_pointer_to_element() {
    let msg = check_err(
        "fn main() {
             ch := sys::chan_new[int]();
             x := 0;
             sys::chan_recv(ch, x);
         }",
    );
    assert!(msg.contains("out must be *int, got int"), "{msg}");
}

#[test]
fn chan_ops_reject_non_channel() {
    let msg = check_err("fn main() { sys::chan_close(5); }");
    assert!(msg.contains("ch must be *Chan[T]"), "{msg}");
    let msg2 = check_err(
        "fn main() {
             a := sys::chan_new[int]();
             b := sys::chan_new[string]();
             sys::chan_send(b, a);
         }",
    );
    assert!(msg2.contains("value must be string, got *Chan__int"), "{msg2}");
}

#[test]
fn chan_annotation_and_opaque_handle() {
    check(
        "fn pull(ch: *Chan[int]) -> int {
             x := 0;
             if !sys::chan_recv(ch, &x) { return -1; }
             return x;
         }
         fn main() {
             ch := sys::chan_new[int]();
             sys::chan_send(ch, 3);
             assert_eq(pull(ch), 3);
             sys::chan_free(ch);
         }",
    )
    .expect("*Chan[int] annotations accepted");
    let msg = check_err(
        "fn main() {
             ch := sys::chan_new[int]();
             x := ch.lock;
         }",
    );
    assert!(msg.contains("has no field 'lock'"), "{msg}");
}

#[test]
fn chan_and_handles_are_reserved_type_names() {
    let msg = check_err("struct Chan[T] { x: T }");
    assert!(msg.contains("'Chan' is reserved"), "{msg}");
    let msg2 = check_err("struct Thread { h: int }");
    assert!(msg2.contains("reserved"), "{msg2}");
    let msg3 = check_err("struct Mutex { m: int }");
    assert!(msg3.contains("reserved"), "{msg3}");
}

#[test]
fn sleep_ms_requires_int() {
    let msg = check_err("fn main() { sys::sleep_ms(\"x\"); }");
    assert!(msg.contains("ms must be int"), "{msg}");
    check("fn main() { sys::sleep_ms(5); }").expect("int accepted");
}

// ---------------------------------------------------------------------------
// emitted C: shape and zero-bloat

#[test]
fn thread_codegen_splices_trampolines_and_tls_arenas() {
    let c = front(
        "fn work(x: int) { sys::sleep_ms(1); }
         fn main() {
             sys::join(sys::thread(work, 5));
         }",
    );
    // per-thread arenas: thread-local storage markers present
    assert!(c.contains("_WLEL_TLS"), "{c}");
    assert!(c.contains("static _WLEL_TLS WArena* _wlel_cur_arena"), "{c}");
    // TLS mapping: gcc/clang (incl. mingw-w64, where __declspec(thread) is
    // silently ignored by the compiler and the arenas would end up shared
    // between threads) must get __thread; __declspec(thread) is MSVC-only
    assert!(
        c.contains("#if defined(__GNUC__) || defined(__clang__)\n#define _WLEL_TLS __thread\n#else\n#define _WLEL_TLS __declspec(thread)\n#endif"),
        "{c}"
    );
    // worker trampoline boxes the argument and frees the thread's arena
    assert!(c.contains("typedef struct _wlel_tharg_work { long long data; } _wlel_tharg_work;"), "{c}");
    assert!(c.contains("_wlel_thread_entry_work"), "{c}");
    assert!(c.contains("wlel_arena_free(_wlel_root_arena)"), "{c}");
    // spawn boxes the argument through malloc
    assert!(
        c.contains("_wlel_thread_spawn(_wlel_thread_entry_work, (const void*)&(_wlel_tharg_work){ .data = 5 }"),
        "{c}"
    );
    // portable platform layer is present
    assert!(c.contains("pthread_create"), "{c}");
    assert!(c.contains("_beginthreadex"), "{c}");
}

#[test]
fn chan_codegen_specializes_per_element_type() {
    let c = front(
        "fn main() {
             ch := sys::chan_new[int]();
             sys::chan_send(ch, 5);
             x := 0;
             assert(sys::chan_recv(ch, &x));
             s := sys::chan_new[string]();
             sys::chan_send(s, \"a\");
             sys::chan_free(s);
             sys::chan_free(ch);
         }",
    );
    assert!(c.contains("struct Chan__int { _wlel_mtx lock;"), "{c}");
    assert!(c.contains("static _Bool _wlel_chan_send_int(Chan__int* ch, long long v)"), "{c}");
    assert!(c.contains("static _Bool _wlel_chan_recv_int(Chan__int* ch, long long* out)"), "{c}");
    assert!(c.contains("struct Chan__string { _wlel_mtx lock;"), "{c}");
    assert!(c.contains("static _Bool _wlel_chan_send_string(Chan__string* ch, const char* v)"), "{c}");
}

#[test]
fn concurrency_runtime_not_spliced_when_unused() {
    let c = front("fn main() { sys::argc(); }");
    assert!(!c.contains("concurrency runtime"), "{c}");
    assert!(!c.contains("pthread_create"), "{c}");
    // safe mode keeps the same property
    let cs = front_safe("fn main() { sys::argc(); }");
    assert!(!cs.contains("concurrency runtime"), "{cs}");
}

#[test]
fn assert_in_spawned_thread_aborts_instead_of_longjmp() {
    // the longjmp guard: _wlel_is_main gates the test-runner jump
    let c = front_safe(
        "fn boom(x: int) { assert(false); }
         fn main() {
             sys::join(sys::thread(boom, 1));
         }",
    );
    assert!(c.contains("if (_wlel_test_active && _wlel_is_main) longjmp"), "{c}");
}

// ---------------------------------------------------------------------------
// end-to-end runs

fn run_cli(tag: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("wlel_conc_{}_{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("prog.wl");
    fs::write(&path, src).expect("write source");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .arg("run")
        .arg(path.to_str().unwrap())
        .output()
        .expect("spawn wlel");
    let out = String::from_utf8_lossy(&run.stdout).into_owned();
    let err = String::from_utf8_lossy(&run.stderr).into_owned();
    (run.status.success(), out, err)
}

#[test]
fn worker_pool_program_computes_exact_total() {
    let (ok, out, err) = run_cli(
        "pool",
        r#"use std;
           fn worker(p: *Pool) {
               job := 0;
               while sys::chan_recv(p.jobs, &job) {
                   sys::mutex_lock(p.mu);
                   *p.total = *p.total + job * job;
                   sys::mutex_unlock(p.mu);
                   sys::chan_send(p.done, job);
               }
           }
           struct Pool {
               jobs: *Chan[int],
               done: *Chan[int],
               mu: *Mutex,
               total: *int,
           }
           fn main() {
               jobs := sys::chan_new[int]();
               done := sys::chan_new[int]();
               mu := sys::mutex_new();
               total := new(int);
               *total = 0;
               pool := Pool { jobs: jobs, done: done, mu: mu, total: total };
               w1 := sys::thread(worker, &pool);
               w2 := sys::thread(worker, &pool);
               w3 := sys::thread(worker, &pool);
               for j in 1..65 {
                   sys::chan_send(jobs, j);
               }
               sys::chan_close(jobs);
               for _k in 0..64 {
                   r := 0;
                   if !sys::chan_recv(done, &r) { sys::exit(1); }
               }
               sys::join(w1);
               sys::join(w2);
               sys::join(w3);
               if *total != 64 * 65 * 129 / 6 { sys::exit(2); }
               sys::mutex_free(mu);
               sys::chan_free(jobs);
               sys::chan_free(done);
               std::println_str("pool ok");
           }"#,
    );
    assert!(ok, "{err}{out}");
    assert!(out.contains("pool ok"), "{out}");
}

#[test]
fn recv_blocks_until_thread_sends() {
    let (ok, out, err) = run_cli(
        "block",
        r#"use std;
           fn sender(ch: *Chan[string]) {
               sys::sleep_ms(30);
               sys::chan_send(ch, "late");
           }
           fn main() {
               ch := sys::chan_new[string]();
               sys::join(sys::thread(sender, ch));
               x := "";
               if !sys::chan_recv(ch, &x) { sys::exit(1); }
               if x != "late" { sys::exit(2); }
               sys::chan_free(ch);
               std::println_str("block ok");
           }"#,
    );
    assert!(ok, "{err}{out}");
    assert!(out.contains("block ok"), "{out}");
}

#[test]
fn example_concurrency_test_blocks_pass() {
    let test = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["test", "examples/concurrency.wl"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawn wlel");
    assert!(test.status.success(), "{}", String::from_utf8_lossy(&test.stderr));
    let out = String::from_utf8_lossy(&test.stdout);
    assert!(out.contains("5 passed, 0 failed"), "{out}");
}

#[test]
fn example_concurrency_demo_runs() {
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["run", "examples/concurrency.wl"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("spawn wlel");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(out.contains("pool ok"), "{out}");
}

// ---------------------------------------------------------------------------
// safe-debug mode: dev checks stay active inside thread bodies

#[test]
fn div_zero_inside_thread_reports_wlel_position() {
    let dir = std::env::temp_dir().join(format!("wlel_concdiv_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("div.wl");
    fs::write(
        &path,
        r#"use std;
           fn boom(x: int) {
               y := x / 0;
               std::println_int(y);
           }
           fn main() {
               sys::join(sys::thread(boom, 5));
           }"#,
    )
    .expect("write source");
    let run = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .arg("run")
        .arg(path.to_str().unwrap())
        .output()
        .expect("spawn wlel");
    assert!(!run.status.success(), "div-by-zero thread must abort");
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(err.contains("division by zero at"), "{err}");
    assert!(err.contains("div.wl:3"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// memory safety: the pool must be LeakSanitizer-clean end to end

#[cfg(unix)]
#[test]
fn worker_pool_is_asan_clean() {
    let dir = std::env::temp_dir().join(format!("wlel_concasan_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("asan.wl");
    fs::write(
        &path,
        r#"use std;
           fn worker(p: *Pool) {
               job := 0;
               while sys::chan_recv(p.jobs, &job) {
                   sys::mutex_lock(p.mu);
                   *p.total = *p.total + job;
                   sys::mutex_unlock(p.mu);
                   sys::chan_send(p.done, job);
               }
           }
           struct Pool {
               jobs: *Chan[int],
               done: *Chan[int],
               mu: *Mutex,
               total: *int,
           }
           fn main() {
               jobs := sys::chan_new[int]();
               done := sys::chan_new[int]();
               mu := sys::mutex_new();
               total := new(int);
               *total = 0;
               pool := Pool { jobs: jobs, done: done, mu: mu, total: total };
               w1 := sys::thread(worker, &pool);
               w2 := sys::thread(worker, &pool);
               for j in 0..200 {
                   sys::chan_send(jobs, j);
               }
               sys::chan_close(jobs);
               for _k in 0..200 {
                   r := 0;
                   if !sys::chan_recv(done, &r) { sys::exit(1); }
               }
               sys::join(w1);
               sys::join(w2);
               if *total != 19900 { sys::exit(2); }
               sys::mutex_free(mu);
               sys::chan_free(jobs);
               sys::chan_free(done);
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

// ---------------------------------------------------------------------------
// formatting: the example is canonical

#[test]
fn example_is_fmt_canonical() {
    let src = fs::read_to_string("examples/concurrency.wl").expect("read example");
    let formatted = wlel::fmt::format_source(&src).expect("example parses");
    assert_eq!(formatted, src, "examples/concurrency.wl is not fmt-canonical");
}
