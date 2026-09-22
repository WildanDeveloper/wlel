use crate::ast::*;
use crate::span::Spanned;

/// Transpiles a Wlel program to C99 source.
pub fn gen_program(p: &Program) -> String {
    let mut cg = Cg {
        out: String::new(),
        defers: Vec::new(),
        loop_defer_bases: Vec::new(),
        current_ret_c: "void".into(),
        main_void: false,
        src_file: String::new(),
        emit_file: String::new(),
        emit_line: 0,
        safe: false,
        used_arr_at: false,
        used_divz: false,
        used_conc: false,
        format_sigs: Vec::new(),
        sorts: Vec::new(),
        bsearches: Vec::new(),
        sort_helpers_at: 0,
        fs_helpers_at: 0,
        conc_at: 0,
        conc_early_at: 0,
        fs_helpers: Vec::new(),
        threads: Vec::new(),
        chans: Vec::new(),
        tmp_id: 0,
    };
    gen_program_into(&mut cg, p, false);
    cg.out
}

/// Transpiles in safe-debug mode: array bounds checks, integer division-by-
/// zero checks and arena poison-fill are emitted alongside the program.
/// This is what `wlel run` and `wlel test` compile with; `wlel build` uses
/// the plain release form (zero overhead).
pub fn gen_program_safe(p: &Program) -> String {
    let mut cg = Cg {
        out: String::new(),
        defers: Vec::new(),
        loop_defer_bases: Vec::new(),
        current_ret_c: "void".into(),
        main_void: false,
        src_file: String::new(),
        emit_file: String::new(),
        emit_line: 0,
        safe: true,
        used_arr_at: false,
        used_divz: false,
        used_conc: false,
        format_sigs: Vec::new(),
        sorts: Vec::new(),
        bsearches: Vec::new(),
        sort_helpers_at: 0,
        fs_helpers_at: 0,
        conc_at: 0,
        conc_early_at: 0,
        fs_helpers: Vec::new(),
        threads: Vec::new(),
        chans: Vec::new(),
        tmp_id: 0,
    };
    gen_program_into(&mut cg, p, false);
    cg.out
}

/// Transpiles for `wlel test`: user `main` is skipped, every `test` block
/// becomes a C function, and the generated main runs them one by one,
/// catching assertion failures via setjmp/longjmp.
pub fn gen_program_tests(p: &Program) -> String {
    let mut cg = Cg {
        out: String::new(),
        defers: Vec::new(),
        loop_defer_bases: Vec::new(),
        current_ret_c: "void".into(),
        main_void: false,
        src_file: String::new(),
        emit_file: String::new(),
        emit_line: 0,
        safe: false,
        used_arr_at: false,
        used_divz: false,
        used_conc: false,
        format_sigs: Vec::new(),
        sorts: Vec::new(),
        bsearches: Vec::new(),
        sort_helpers_at: 0,
        fs_helpers_at: 0,
        conc_at: 0,
        conc_early_at: 0,
        fs_helpers: Vec::new(),
        threads: Vec::new(),
        chans: Vec::new(),
        tmp_id: 0,
    };
    gen_program_into(&mut cg, p, true);
    cg.out
}

/// `wlel test` counterpart of `gen_program_safe`.
pub fn gen_program_tests_safe(p: &Program) -> String {
    let mut cg = Cg {
        out: String::new(),
        defers: Vec::new(),
        loop_defer_bases: Vec::new(),
        current_ret_c: "void".into(),
        main_void: false,
        src_file: String::new(),
        emit_file: String::new(),
        emit_line: 0,
        safe: true,
        used_arr_at: false,
        used_divz: false,
        used_conc: false,
        format_sigs: Vec::new(),
        sorts: Vec::new(),
        bsearches: Vec::new(),
        sort_helpers_at: 0,
        fs_helpers_at: 0,
        conc_at: 0,
        conc_early_at: 0,
        fs_helpers: Vec::new(),
        threads: Vec::new(),
        chans: Vec::new(),
        tmp_id: 0,
    };
    gen_program_into(&mut cg, p, true);
    cg.out
}

/// a registered defer: a single void expression or a block of statements
#[derive(Clone)]
enum DeferItem {
    Expr(Expr),
    Block(Vec<Stmt>),
}

struct Cg {
    out: String,
    /// defers registered by enclosing blocks, in registration order
    defers: Vec<DeferItem>,
    /// defer-stack depth at the start of each enclosing loop body;
    /// break/continue must run defers registered after this point
    loop_defer_bases: Vec<usize>,
    /// C return type of current function (for return-expr tmp variable)
    current_ret_c: String,
    /// wlel `main` declared void: C requires `int main`, so bare returns
    /// become `return 0;`
    main_void: bool,
    /// source file of the function currently being generated
    src_file: String,
    /// file/line the last emitted `#line` directive points to
    emit_file: String,
    emit_line: usize,
    /// safe-debug mode: emit bounds checks, div-zero checks and arena
    /// poison-fill (`wlel run`/`wlel test`); release builds set this off
    safe: bool,
    /// safe-mode helpers actually called by the generated code; their
    /// definitions are spliced into the prelude once, only when used
    used_arr_at: bool,
    used_divz: bool,
    /// concurrency layer (threads/mutexes/channels) in use; the whole
    /// runtime is spliced only when the program actually touches it
    used_conc: bool,
    /// `std::format` call signatures seen while generating ("iis" = int,
    /// int, string); one tiny C wrapper per distinct signature is spliced
    /// into the prelude after generation, only for signatures in use
    format_sigs: Vec<String>,
    /// (element type, comparator) pairs seen on `std::sort` calls; one
    /// specialized quicksort per pair is spliced in after the function
    /// prototypes, only for pairs in use
    sorts: Vec<(String, String)>,
    /// (element type, comparator) pairs seen on `std::binary_search` calls
    bsearches: Vec<(String, String)>,
    /// splice offset for the sort/search helpers: after the struct
    /// definitions and function prototypes (they reference both), before
    /// the function bodies that call them
    sort_helpers_at: usize,
    // splice offset for the fs Result helpers: right after the type
    // definitions (they return std::Result enums by value), before the
    // prototypes
    fs_helpers_at: usize,
    /// splice offset for the channel structs + functions: same boundary as
    /// the fs helpers — after the type definitions, since a Chan element may
    /// embed a user struct by value
    conc_at: usize,
    /// splice offset for the base concurrency runtime: right after the
    /// forward typedefs, before the type definitions — user structs may
    /// carry *Thread/*Mutex/*Chan fields
    conc_early_at: usize,
    /// fs Result helpers actually called by the generated code; their
    /// definitions (and only those) are spliced in after generation
    fs_helpers: Vec<&'static str>,
    /// (worker fn, argument type name) pairs seen on `sys::thread`; one
    /// boxed-argument trampoline per pair is spliced in after the prototypes
    threads: Vec<(String, String)>,
    /// channel element type names seen on `sys::chan_*` calls; one runtime
    /// struct + function set per element type is spliced in with the
    /// concurrency layer
    chans: Vec<String>,
    /// counter for fresh compiler temporaries (`_wlel_it0`, ...)
    tmp_id: usize,
}

fn gen_program_into(cg: &mut Cg, p: &Program, test_mode: bool) {
    cg.out.push_str("/* generated by wlel */\n");
    cg.out.push_str("#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\n#include <stddef.h>\n#include <setjmp.h>\n#include <time.h>\n#include <math.h>\n#include <errno.h>\n\n");
    // thread-local storage for the per-thread arenas; the Windows side of
    // the concurrency runtime also pulls in windows.h here. MinGW gcc does
    // not implement __declspec(thread) (silently ignored with a warning,
    // leaving the arenas shared across threads) but does support __thread
    // via emutls — so declspec is reserved for MSVC only.
    cg.out.push_str("#ifdef _WIN32\n#include <windows.h>\n#endif\n#if defined(__GNUC__) || defined(__clang__)\n#define _WLEL_TLS __thread\n#else\n#define _WLEL_TLS __declspec(thread)\n#endif\n\n");
    // Windows: put stdout/stderr in binary mode so a program's output is
    // byte-identical across OSes (printf("\n") must not become \r\n)
    cg.out.push_str("#ifdef _WIN32\n#include <io.h>\n#include <fcntl.h>\nstatic void _wlel_binary_stdout(void) { _setmode(_fileno(stdout), _O_BINARY); _setmode(_fileno(stderr), _O_BINARY); }\n#else\nstatic void _wlel_binary_stdout(void) {}\n#endif\n\n");
    // runtime builtins
    cg.out.push_str("static void wlel_print_int(long long v) { printf(\"%lld\\n\", v); }\n");
    cg.out.push_str("static void wlel_print_float(double v) { printf(\"%g\\n\", v); }\n");
    cg.out.push_str("static void wlel_print_str(const char* s) { fputs(s, stdout); }\n\n");
    cg.out.push_str("#include <stdlib.h>\n\n");
    cg.out.push_str("static void std__print_int(long long v) { printf(\"%lld\", v); }\n");
    cg.out.push_str("static void std__print_float(double v) { printf(\"%g\", v); }\n");
    cg.out.push_str("static void std__print_str(const char* s) { fputs(s, stdout); }\n");
    cg.out.push_str("static void std__println_int(long long v) { printf(\"%lld\\n\", v); }\n");
    cg.out.push_str("static void std__println_float(double v) { printf(\"%g\\n\", v); }\n");
    cg.out.push_str("static void std__println_str(const char* s) { printf(\"%s\\n\", s); }\n");
    cg.out.push_str("static long long std__strlen(const char* s) { return (long long)strlen(s); }\n");
    cg.out.push_str("static _Bool std__streq(const char* a, const char* b) { return strcmp(a, b) == 0; }\n");
    cg.out.push_str("static _Bool _wlel_streq(const char* a, const char* b) { return strcmp(a, b) == 0; }\n");
    cg.out.push_str("static long long std__abs(long long v) { return v < 0 ? -v : v; }\n");
    cg.out.push_str("static long long std__min(long long a, long long b) { return a < b ? a : b; }\n");
    cg.out.push_str("static long long std__max(long long a, long long b) { return a > b ? a : b; }\n");
    // checked arithmetic: builtins use the CPU overflow flag (no UB, no wrap
    // assumption); on overflow the WRAPPED result is still stored in *out —
    // only the bool tells you whether the mathematical result fit
    cg.out.push_str("static _Bool std__checked_add(long long a, long long b, long long* out) { return !__builtin_add_overflow(a, b, out); }\n");
    cg.out.push_str("static _Bool std__checked_sub(long long a, long long b, long long* out) { return !__builtin_sub_overflow(a, b, out); }\n");
    cg.out.push_str("static _Bool std__checked_mul(long long a, long long b, long long* out) { return !__builtin_mul_overflow(a, b, out); }\n\n");
    cg.out.push_str("static int _wlel_argc = 0;\n");
    cg.out.push_str("static char** _wlel_argv = 0;\n");
    cg.out.push_str("static long long sys__argc(void) { return (long long)_wlel_argc; }\n");
    cg.out.push_str("static const char* sys__arg(long long i) { if (i < 0 || i >= _wlel_argc) return \"\"; return _wlel_argv[i]; }\n");
    cg.out.push_str("static void sys__exit(long long code) { exit((int)code); }\n\n");
    // wall clocks in milliseconds: monotonic for measuring durations (never
    // jumps), realtime for timestamps
    cg.out.push_str("static long long sys__mono_ms(void) {\n");
    cg.out.push_str("    struct timespec ts;\n");
    cg.out.push_str("    clock_gettime(CLOCK_MONOTONIC, &ts);\n");
    cg.out.push_str("    return (long long)ts.tv_sec * 1000 + (long long)ts.tv_nsec / 1000000;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static long long sys__unix_ms(void) {\n");
    cg.out.push_str("    struct timespec ts;\n");
    cg.out.push_str("    clock_gettime(CLOCK_REALTIME, &ts);\n");
    cg.out.push_str("    return (long long)ts.tv_sec * 1000 + (long long)ts.tv_nsec / 1000000;\n");
    cg.out.push_str("}\n\n");
    // std::math::* — thin wrappers over C99 libm; the mangled call names
    // (std::math::sqrt -> std__math__sqrt) resolve here
    for (w, args) in [
        ("sqrt", "x"), ("cbrt", "x"), ("exp", "x"), ("log", "x"), ("log2", "x"),
        ("log10", "x"), ("sin", "x"), ("cos", "x"), ("tan", "x"), ("asin", "x"),
        ("acos", "x"), ("atan", "x"), ("sinh", "x"), ("cosh", "x"), ("tanh", "x"),
        ("floor", "x"), ("ceil", "x"), ("round", "x"), ("trunc", "x"), ("fabs", "x"),
    ] {
        cg.out.push_str(&format!(
            "static double std__math__{w}(double {args}) {{ return {w}({args}); }}\n"
        ));
    }
    for (w, a, b) in [
        ("pow", "b", "e"), ("atan2", "y", "x"), ("fmin", "a", "b"),
        ("fmax", "a", "b"), ("hypot", "a", "b"), ("fmod", "a", "b"),
    ] {
        cg.out.push_str(&format!(
            "static double std__math__{w}(double {a}, double {b}) {{ return {w}({a}, {b}); }}\n"
        ));
    }
    cg.out.push('\n');
    // std::random::* — xorshift64* PRNG. The state auto-seeds lazily from
    // the monotonic clock plus stack-address entropy on first use; seed()
    // fixes the stream for reproducible runs. A zero state is the xorshift
    // fixed point, so both paths force a nonzero state.
    cg.out.push_str("static unsigned long long _wlel_rng_state = 0;\n");
    cg.out.push_str("static unsigned long long _wlel_rng_mix(unsigned long long z) {\n");
    cg.out.push_str("    z ^= z >> 33; z *= 0xff51afd7ed558ccdULL; z ^= z >> 33; z *= 0xc4ceb9fe1a85ec53ULL; z ^= z >> 33;\n");
    cg.out.push_str("    return z;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static void std__random__seed(long long s) {\n");
    cg.out.push_str("    unsigned long long x = _wlel_rng_mix((unsigned long long)s ^ 0x9E3779B97F4A7C15ULL);\n");
    cg.out.push_str("    _wlel_rng_state = x ? x : 0x9E3779B97F4A7C15ULL;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static unsigned long long std__random__next(void) {\n");
    cg.out.push_str("    if (_wlel_rng_state == 0) {\n");
    cg.out.push_str("        struct timespec ts;\n");
    cg.out.push_str("        clock_gettime(CLOCK_MONOTONIC, &ts);\n");
    cg.out.push_str("        unsigned long long mixed = _wlel_rng_mix(((unsigned long long)ts.tv_sec << 20) ^ (unsigned long long)ts.tv_nsec ^ _wlel_rng_mix((unsigned long long)&ts));\n");
    cg.out.push_str("        _wlel_rng_state = mixed ? mixed : 0x9E3779B97F4A7C15ULL;\n");
    cg.out.push_str("    }\n");
    cg.out.push_str("    unsigned long long x = _wlel_rng_state;\n");
    cg.out.push_str("    x ^= x >> 12; x ^= x << 25; x ^= x >> 27;\n");
    cg.out.push_str("    _wlel_rng_state = x;\n");
    cg.out.push_str("    return x * 0x2545F4914F6CDD1DULL;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static long long std__random__int(long long n) {\n");
    cg.out.push_str("    if (n <= 0) return 0;\n");
    cg.out.push_str("    return (long long)(std__random__next() % (unsigned long long)n);\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static double std__random__float(void) {\n");
    cg.out.push_str("    return (double)(std__random__next() >> 11) * (1.0 / 9007199254740992.0);\n");
    cg.out.push_str("}\n\n");
    // assertion primitive: inside a test run it longjmps back to the runner
    // so the suite continues; otherwise it aborts with the source location.
    // _wlel_is_main guards the longjmp: an assert inside a spawned thread
    // must abort the process, never jump into a (foreign) test stack
    cg.out.push_str("static _WLEL_TLS int _wlel_is_main = 0;\n");
    cg.out.push_str("static jmp_buf _wlel_test_jmp;\n");
    cg.out.push_str("static int _wlel_test_active = 0;\n");
    cg.out.push_str("static const char* _wlel_fail_file = \"?\";\n");
    cg.out.push_str("static long long _wlel_fail_line = 0;\n");
    cg.out.push_str("static void _wlel_assert(_Bool cond, const char* file, long long line) {\n");
    cg.out.push_str("    if (cond) return;\n");
    cg.out.push_str("    _wlel_fail_file = file; _wlel_fail_line = line;\n");
    cg.out.push_str("    if (_wlel_test_active && _wlel_is_main) longjmp(_wlel_test_jmp, 1);\n");
    cg.out.push_str("    fprintf(stderr, \"assertion failed at %s:%lld\\n\", file, line);\n");
    cg.out.push_str("    exit(1);\n");
    cg.out.push_str("}\n\n");
    // panic(msg): like a failed assert, but with the caller's message; a
    // test run longjmps back to the runner so the suite continues
    cg.out.push_str("static void _wlel_panic(const char* msg, const char* file, long long line) {\n");
    cg.out.push_str("    fprintf(stderr, \"panic at %s:%lld: %s\\n\", file, line, msg);\n");
    cg.out.push_str("    _wlel_fail_file = file; _wlel_fail_line = line;\n");
    cg.out.push_str("    if (_wlel_test_active && _wlel_is_main) longjmp(_wlel_test_jmp, 1);\n");
    cg.out.push_str("    exit(1);\n");
    cg.out.push_str("}\n\n");
    cg.out.push_str("typedef struct WArena { unsigned char* buf; unsigned long long cap; unsigned long long used; unsigned long long total; unsigned long long peak; struct WArena* next; } WArena;\n");
    // built-in stats struct for arena_stats(); 'ArenaStats' is reserved by
    // the checker, so this never collides with a user struct
    cg.out.push_str("typedef struct ArenaStats { long long bytes; long long chunks; long long peak; } ArenaStats;\n");
    cg.out.push_str("static void* wlel_alloc(unsigned long long n) { return malloc(n); }\n");
    cg.out.push_str("static void wlel_free(void* p) { free(p); }\n");
    cg.out.push_str("static WArena* wlel_arena_new(unsigned long long cap) {\n");
    cg.out.push_str("    WArena* a = (WArena*)malloc(sizeof(WArena));\n");
    cg.out.push_str("    a->buf = (unsigned char*)malloc(cap ? cap : 1);\n");
    cg.out.push_str("    a->cap = cap ? cap : 1; a->used = 0; a->total = 0; a->peak = 0; a->next = 0;\n");
    cg.out.push_str("    return a;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static void* wlel_arena_alloc(WArena* a, unsigned long long n) {\n");
    // every allocation starts at an 8-byte boundary: the widest Wlel types
    // (i64/u64/f64, structs of them) require it, and string allocations of
    // arbitrary length would otherwise leave struct buffers misaligned
    cg.out.push_str("    a->used = (a->used + 7ULL) & ~7ULL;\n");
    cg.out.push_str("    if (a->used + n <= a->cap) { void* p = a->buf + a->used; a->used += n; a->total += n; if (a->total > a->peak) a->peak = a->total; return p; }\n");
    cg.out.push_str("    WArena* chunk = (WArena*)malloc(sizeof(WArena));\n");
    cg.out.push_str("    unsigned long long cap = n > a->cap ? n : a->cap;\n");
    cg.out.push_str("    chunk->buf = (unsigned char*)malloc(cap); chunk->cap = cap; chunk->used = n; chunk->total = n; chunk->peak = n; chunk->next = a->next;\n");
    cg.out.push_str("    a->total += n; if (a->total > a->peak) a->peak = a->total;\n");
    cg.out.push_str("    a->next = chunk;\n");
    cg.out.push_str("    return chunk->buf;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static void wlel_arena_free(WArena* a) {\n");
    if cg.safe {
        // debug aid: fill freed chunks with 0xDE so use-after-free reads
        // deterministic garbage (0xDEDE...) instead of silent old data
        cg.out
            .push_str("    while (a) { WArena* nx = a->next; memset(a->buf, 0xDE, a->cap); free(a->buf); free(a); a = nx; }\n");
    } else {
        cg.out
            .push_str("    while (a) { WArena* nx = a->next; free(a->buf); free(a); a = nx; }\n");
    }
    cg.out.push_str("}\n");
    // arena state is thread-local: every Wlel thread owns a root arena
    // (spawned threads get a fresh one in their trampoline), so a bump
    // allocation never races
    cg.out.push_str("static _WLEL_TLS WArena* _wlel_cur_arena = 0;\n");
    cg.out.push_str("static _WLEL_TLS WArena* _wlel_root_arena = 0;\n");
    cg.out.push_str("static void __wlel_arena_exit(WArena* prev) {\n");
    cg.out.push_str("    wlel_arena_free(_wlel_cur_arena);\n");
    cg.out.push_str("    _wlel_cur_arena = prev;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static void __wlel_root_cleanup(void) {\n");
    cg.out.push_str("    if (_wlel_root_arena) { wlel_arena_free(_wlel_root_arena); _wlel_root_arena = 0; _wlel_cur_arena = 0; }\n");
    cg.out.push_str("}\n");
    // arena_stats(): bytes/chunks of the active arena's chunk chain, plus
    // the high-water mark tracked incrementally on the head chunk
    cg.out.push_str("static ArenaStats _wlel_arena_stats(void) {\n");
    cg.out.push_str("    ArenaStats s; s.bytes = 0; s.chunks = 0; s.peak = 0;\n");
    cg.out.push_str("    for (WArena* a = _wlel_cur_arena; a; a = a->next) { s.bytes += (long long)a->used; s.chunks += 1; }\n");
    cg.out.push_str("    if (_wlel_cur_arena) s.peak = (long long)_wlel_cur_arena->peak;\n");
    cg.out.push_str("    return s;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static char* _wlel_strcat(const char* a, const char* b) {\n");
    cg.out.push_str("    unsigned long long la = strlen(a), lb = strlen(b);\n");
    cg.out.push_str("    char* buf = (char*)(_wlel_cur_arena ? wlel_arena_alloc(_wlel_cur_arena, la + lb + 1) : malloc(la + lb + 1));\n");
    cg.out.push_str("    memcpy(buf, a, la); memcpy(buf + la, b, lb); buf[la + lb] = 0;\n");
    cg.out.push_str("    return buf;\n");
    cg.out.push_str("}\n\n");
    // string library runtime: every produced string lands in the ACTIVE
    // arena (or the heap when none is open), matching `+` concat semantics
    cg.out.push_str("static char* _wlel_alloc_str(unsigned long long n) {\n");
    cg.out.push_str("    return (char*)(_wlel_cur_arena ? wlel_arena_alloc(_wlel_cur_arena, n) : malloc(n));\n");
    cg.out.push_str("}\n");
    // file I/O runtime. The File wrapper lives in the ACTIVE arena (or the
    // heap outside any arena), so handles die with the block that opened
    // them and LeakSanitizer stays clean; close() only closes the handle,
    // making it idempotent and defer-safe.
    cg.out.push_str("typedef struct File { void* h; } File;\n");
    cg.out.push_str("static _Bool _wlel_read_file(const char* path, const char** out) {\n");
    cg.out.push_str("    FILE* f = fopen(path, \"rb\");\n");
    cg.out.push_str("    if (!f) return 0;\n");
    cg.out.push_str("    if (fseek(f, 0, SEEK_END) != 0) { fclose(f); return 0; }\n");
    cg.out.push_str("    long n = ftell(f);\n");
    cg.out.push_str("    if (n < 0) { fclose(f); return 0; }\n");
    cg.out.push_str("    if (fseek(f, 0, SEEK_SET) != 0) { fclose(f); return 0; }\n");
    cg.out.push_str("    char* buf = _wlel_alloc_str((unsigned long long)n + 1);\n");
    cg.out.push_str("    size_t got = n > 0 ? fread(buf, 1, (size_t)n, f) : 0;\n");
    cg.out.push_str("    int bad = ferror(f);\n");
    cg.out.push_str("    fclose(f);\n");
    cg.out.push_str("    if (bad || got != (size_t)n) return 0;\n");
    cg.out.push_str("    buf[n] = 0;\n");
    cg.out.push_str("    *out = buf;\n");
    cg.out.push_str("    return 1;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static _Bool _wlel_write_file(const char* path, const char* contents) {\n");
    cg.out.push_str("    FILE* f = fopen(path, \"wb\");\n");
    cg.out.push_str("    if (!f) return 0;\n");
    cg.out.push_str("    size_t n = strlen(contents);\n");
    cg.out.push_str("    size_t w = fwrite(contents, 1, n, f);\n");
    cg.out.push_str("    int bad = ferror(f);\n");
    cg.out.push_str("    if (fclose(f) != 0) return 0;\n");
    cg.out.push_str("    return !bad && w == n;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static void _wlel_fs_close(File* f) {\n");
    cg.out.push_str("    if (f && f->h) { fclose((FILE*)f->h); f->h = 0; }\n");
    cg.out.push_str("}\n");
    // strict float parse: strtod, but the whole string must be consumed
    // ("1.5x" is rejected; leading whitespace is accepted by strtod)
    cg.out.push_str("static _Bool std__parse_float(const char* s, double* out) {\n");
    cg.out.push_str("    char* end;\n");
    cg.out.push_str("    double v = strtod(s, &end);\n");
    cg.out.push_str("    if (end == s || *end != 0) return 0;\n");
    cg.out.push_str("    *out = v;\n");
    cg.out.push_str("    return 1;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static char* std__float_to_str(double v) {\n");
    cg.out.push_str("    char tmp[64];\n");
    cg.out.push_str("    int n = snprintf(tmp, sizeof tmp, \"%g\", v);\n");
    cg.out.push_str("    char* buf = _wlel_alloc_str((unsigned long long)n + 1);\n");
    cg.out.push_str("    snprintf(buf, (unsigned long long)n + 1, \"%g\", v);\n");
    cg.out.push_str("    return buf;\n");
    cg.out.push_str("}\n");
    // std::format core: copy fmt, replacing each "{}" with the next part;
    // unmatched braces and surplus placeholders stay literal, surplus
    // arguments are ignored (printf-style)
    cg.out.push_str("static char* _wlel_format_join(const char* fmt, const char** parts, long long nparts) {\n");
    cg.out.push_str("    long long used = 0;\n");
    cg.out.push_str("    long long total = 0;\n");
    cg.out.push_str("    for (const char* p = fmt; *p; p++) {\n");
    cg.out.push_str("        if (p[0] == '{' && p[1] == '}' && used < nparts) { total += (long long)strlen(parts[used]); used += 1; p += 1; }\n");
    cg.out.push_str("        else { total += 1; }\n");
    cg.out.push_str("    }\n");
    cg.out.push_str("    char* buf = _wlel_alloc_str((unsigned long long)total + 1);\n");
    cg.out.push_str("    char* w = buf;\n");
    cg.out.push_str("    used = 0;\n");
    cg.out.push_str("    for (const char* p = fmt; *p; p++) {\n");
    cg.out.push_str("        if (p[0] == '{' && p[1] == '}' && used < nparts) {\n");
    cg.out.push_str("            const char* s = parts[used++];\n");
    cg.out.push_str("            long long l = (long long)strlen(s);\n");
    cg.out.push_str("            memcpy(w, s, (unsigned long long)l);\n");
    cg.out.push_str("            w += l;\n");
    cg.out.push_str("            p += 1;\n");
    cg.out.push_str("        } else {\n");
    cg.out.push_str("            *w++ = *p;\n");
    cg.out.push_str("        }\n");
    cg.out.push_str("    }\n");
    cg.out.push_str("    *w = 0;\n");
    cg.out.push_str("    return buf;\n");
    cg.out.push_str("}\n\n");
    // hashing primitives for HashMap keys (chosen by the checker per key type)
    cg.out.push_str("static unsigned long long _wlel_hash_i64(long long v) {\n");
    cg.out.push_str("    unsigned long long x = (unsigned long long)v;\n");
    cg.out.push_str("    x ^= x >> 33; x *= 0xff51afd7ed558ccdULL; x ^= x >> 33; x *= 0xc4ceb9fe1a85ec53ULL; x ^= x >> 33;\n");
    cg.out.push_str("    return x;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static unsigned long long _wlel_hash_str(const char* s) {\n");
    cg.out.push_str("    unsigned long long h = 1469598103934665603ULL;\n");
    cg.out.push_str("    for (const unsigned char* p = (const unsigned char*)s; *p; p++) { h ^= *p; h *= 1099511628211ULL; }\n");
    cg.out.push_str("    return h;\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static unsigned long long _wlel_hash_f64(double v) {\n");
    cg.out.push_str("    unsigned long long x; memcpy(&x, &v, sizeof(x)); return _wlel_hash_i64((long long)x);\n");
    cg.out.push_str("}\n");
    cg.out.push_str("static unsigned long long _wlel_hash_ptr(const void* p) { return _wlel_hash_i64((long long)(unsigned long long)p); }\n\n");
    // safe-debug runtime helpers are spliced in HERE (before any use) after
    // generation finishes, and only when the generated code calls them —
    // unused debug scaffolding never reaches the emitted C; `std::format`
    // wrappers land at the same boundary, one per distinct call signature
    let safe_helpers_at = cg.out.len();
    let format_helpers_at = safe_helpers_at;

    // forward struct declarations so self-referential / mutually-referential
    // pointer fields compile cleanly in C
    for st in &p.structs {
        cg.out.push_str(&format!("typedef struct {} {};\n", st.name, st.name));
    }
    // enums share the C struct namespace: forward-declare them too
    for e in &p.enums {
        cg.out.push_str(&format!("typedef struct {} {};\n", e.name, e.name));
    }
    cg.out.push('\n');

    // the base concurrency runtime (platform layer, Thread/Mutex handles,
    // spawn/join/mutex/sleep functions and forward typedefs for every
    // channel instance) splices in HERE: user structs with *Chan/*Mutex
    // fields are emitted right after, so the names must exist first
    cg.conc_early_at = cg.out.len();

    // struct + enum definitions, ordered so by-value members are complete
    // at their point of definition: an enum payload may embed a struct by
    // value, and a struct field may embed an enum by value
    emit_types(cg, p);

    // fs Result helpers + the concurrency runtime splice in HERE: the
    // std::Result enum instances the fs helpers return by value are defined
    // above, and a Chan element may embed a user struct by value, so both
    // land after the type definitions and before the prototypes that take
    // *Mutex / *Chan / *Thread parameters
    cg.fs_helpers_at = cg.out.len();
    cg.conc_at = cg.out.len();

    // prototypes so forward references link correctly; extern declarations
    // are plain (non-static) prototypes — they resolve to external C symbols
    for f in &p.funcs {
        if f.name == "main" {
            continue;
        }
        let ret = c_type(f.ret_type.as_deref().unwrap_or("void"));
        let params: Vec<String> = f
            .params
            .iter()
            .map(|pa| format!("{} {}", c_type(pa.ty.as_deref().unwrap_or("int")), pa.name))
            .collect();
        let linkage = if f.is_extern { "" } else { "static " };
        cg.out.push_str(&format!(
            "{linkage}{} {}({});\n",
            ret,
            f.name,
            params.join(", ")
        ));
    }
    cg.out.push('\n');
    // sort/search helpers splice in HERE: struct definitions and the
    // prototypes above are exactly what they reference (element structs,
    // comparator functions), and the function bodies below call them
    cg.sort_helpers_at = cg.out.len();

    for f in &p.funcs {
        if test_mode && f.name == "main" {
            // the test runner main replaces the program's entry point
            continue;
        }
        if f.is_extern {
            // declaration only: the prototype above is the whole contract,
            // the linker provides the definition
            continue;
        }
        let ret_c = c_type(f.ret_type.as_deref().unwrap_or("void"));
        let params: Vec<String> = f
            .params
            .iter()
            .map(|pa| format!("{} {}", c_type(pa.ty.as_deref().unwrap_or("int")), pa.name))
            .collect();
        cg.src_file = f.file.clone();
        cg.sync_line(f.span.start.line);
        if f.name == "main" {
            cg.current_ret_c = "int".into();
            cg.main_void = f.ret_type.is_none();
            cg.out.push_str("int main(int _wlel_main_argc, char** _wlel_main_argv) {\n");
            indent(&mut cg.out, 1);
            cg.out.push_str("_wlel_binary_stdout();\n");
            indent(&mut cg.out, 1);
            cg.out.push_str("_wlel_is_main = 1;\n");
            indent(&mut cg.out, 1);
            cg.out.push_str("_wlel_argc = _wlel_main_argc; _wlel_argv = _wlel_main_argv;\n");
            indent(&mut cg.out, 1);
            cg.out.push_str("_wlel_root_arena = wlel_arena_new(65536);\n");
            indent(&mut cg.out, 1);
            cg.out.push_str("_wlel_cur_arena = _wlel_root_arena;\n");
            indent(&mut cg.out, 1);
            cg.out.push_str("atexit(__wlel_root_cleanup);\n");
        } else {
            cg.current_ret_c = ret_c.clone();
            cg.main_void = false;
            cg.out.push_str(&format!("{} {}({}) {{\n", ret_c, f.name, params.join(", ")));
        }
        gen_block(cg, &f.body, 1);
        let ends_with_return = matches!(
            f.body.0.last().map(|s| &s.node),
            Some(StmtKind::Return(_))
        );
        if f.name == "main" && cg.main_void && !ends_with_return {
            indent(&mut cg.out, 1);
            cg.out.push_str("return 0;\n");
        }
        cg.out.push_str("}\n\n");
    }

    if test_mode {
        for (i, t) in p.tests.iter().enumerate() {
            gen_test_fn(cg, i, t);
        }
        gen_test_main(cg, p);
    }

    // one quicksort / binary search per distinct (element, comparator) pair,
    // specialized so the comparator is a direct (inlinable) call. Spliced
    // FIRST: it sits at the largest offset, and every insertion shifts
    // content after its position, so later (smaller-offset) splices keep
    // their captured offsets valid.
    if !cg.sorts.is_empty() || !cg.bsearches.is_empty() {
        let mut h = String::new();
        for (i, (elem, cmp)) in cg.sorts.iter().enumerate() {
            h.push_str(&sort_helper_c(i, elem, cmp));
        }
        for (i, (elem, cmp)) in cg.bsearches.iter().enumerate() {
            h.push_str(&bsearch_helper_c(i, elem, cmp));
        }
        cg.out.insert_str(cg.sort_helpers_at, &h);
    }

    // thread trampolines: one boxed-argument entry per (worker, arg type)
    // pair. They call user functions, so they sit after the prototypes —
    // same splice boundary as the sort helpers. Each trampoline gives the
    // thread its own root arena and frees it before exiting.
    if !cg.threads.is_empty() {
        let mut h = String::new();
        for (work, ty) in &cg.threads {
            h.push_str(&thread_entry_c(work, ty));
        }
        cg.out.insert_str(cg.sort_helpers_at, &h);
    }

    // concurrency runtime, spliced in only when the program actually spawns
    // threads, locks mutexes or sends over channels (zero bloat otherwise).
    // The base layer (platform threads, Thread/Mutex handles, spawn/join/
    // mutex/sleep functions and per-channel forward typedefs) sits BEFORE
    // the user type definitions so a struct can carry *Chan/*Mutex fields;
    // the per-element channel structs + functions sit after them, where the
    // element types are complete.
    if cg.used_conc {
        // splice from the LARGEST offset down: insert_str offsets refer to
        // the current buffer, so a later (smaller-offset) splice keeps its
        // captured position only if bigger ones went in first
        if !cg.chans.is_empty() {
            let mut late = String::new();
            for elem in &cg.chans {
                late.push_str(&chan_helpers_c(elem));
            }
            cg.out.insert_str(cg.conc_at, &late);
        }

        let mut early = String::new();
        early.push_str(&conc_runtime_c());
        for elem in &cg.chans {
            let s = crate::checker::mangle_ty(elem);
            early.push_str(&format!(
                "typedef struct Chan__{s} Chan__{s};\n"
            ));
        }
        cg.out.insert_str(cg.conc_early_at, &early);
    }

    // splice in only the fs Result helpers the generated code calls (each
    // call site was rewritten by the checker to the _wlel_fs_*_r helper)
    if !cg.fs_helpers.is_empty() {
        let mut h = String::new();
        for name in &cg.fs_helpers {
            h.push_str(fs_helper_c(name));
        }
        cg.out.insert_str(cg.fs_helpers_at, &h);
    }

    // splice in only the safe-mode helpers the generated code actually calls
    if cg.safe && (cg.used_arr_at || cg.used_divz) {
        let mut h = String::new();
        if cg.used_arr_at {
            h.push_str("static unsigned char* _wlel_arr_at(void* base, long long n, unsigned long long esz, long long i, const char* f, long long l) {\n");
            h.push_str("    if (i < 0 || i >= n) {\n");
            h.push_str("        fprintf(stderr, \"bounds check failed at %s:%lld: index %lld, length %lld\\n\", f, l, i, n);\n");
            h.push_str("        exit(1);\n");
            h.push_str("    }\n");
            h.push_str("    return (unsigned char*)base + (unsigned long long)i * esz;\n");
            h.push_str("}\n");
        }
        if cg.used_divz {
            h.push_str("static long long _wlel_divz(long long d, const char* f, long long l) {\n");
            h.push_str("    if (d == 0) {\n");
            h.push_str("        fprintf(stderr, \"division by zero at %s:%lld\\n\", f, l);\n");
            h.push_str("        exit(1);\n");
            h.push_str("    }\n");
            h.push_str("    return d;\n");
            h.push_str("}\n");
        }
        cg.out.insert_str(safe_helpers_at, &h);
    }

    // one small wrapper per distinct `std::format` signature: converts each
    // argument to a string, then hands fmt + parts to _wlel_format_join
    if !cg.format_sigs.is_empty() {
        let mut h = String::new();
        for sig in &cg.format_sigs {
            h.push_str(&format_sig_wrapper(sig));
        }
        cg.out.insert_str(format_helpers_at, &h);
    }
}

/// C quicksort specialized for one (element type, comparator) pair: the
/// comparator is called directly, so `-O2` inlines it. Median-of-three
/// pivot, Dijkstra 3-way partition (duplicates land in the middle band,
/// all-equal input finishes in one pass), recursion into the smaller side
/// with the larger side iterated — stack depth stays logarithmic. Not
/// stable, like C qsort.
fn sort_helper_c(idx: usize, elem: &str, cmp: &str) -> String {
    let t = c_type(elem);
    format!(
"static void _wlel_sort_{idx}({t}* a, long long lo, long long hi) {{
    while (lo < hi) {{
        long long mid = lo + (hi - lo) / 2;
        if ({cmp}(a[mid], a[lo]) < 0) {{ {t} tmp = a[mid]; a[mid] = a[lo]; a[lo] = tmp; }}
        if ({cmp}(a[hi], a[lo]) < 0) {{ {t} tmp = a[hi]; a[hi] = a[lo]; a[lo] = tmp; }}
        if ({cmp}(a[hi], a[mid]) < 0) {{ {t} tmp = a[hi]; a[hi] = a[mid]; a[mid] = tmp; }}
        {t} tmp = a[lo]; a[lo] = a[mid]; a[mid] = tmp;
        {t} pivot = a[lo];
        long long lt = lo;
        long long gt = hi;
        long long i = lo + 1;
        while (i <= gt) {{
            long long c = {cmp}(a[i], pivot);
            if (c < 0) {{
                {t} tmp = a[lt]; a[lt] = a[i]; a[i] = tmp;
                lt += 1;
                i += 1;
            }} else if (c > 0) {{
                {t} tmp = a[i]; a[i] = a[gt]; a[gt] = tmp;
                gt -= 1;
            }} else {{
                i += 1;
            }}
        }}
        if (lt - lo < hi - gt) {{
            _wlel_sort_{idx}(a, lo, lt - 1);
            lo = gt + 1;
        }} else {{
            _wlel_sort_{idx}(a, gt + 1, hi);
            hi = lt - 1;
        }}
    }}
}}
")
}

/// C binary search specialized like the sort helper: returns an index of a
/// match, -1 when the key is absent.
fn bsearch_helper_c(idx: usize, elem: &str, cmp: &str) -> String {
    let t = c_type(elem);
    format!(
"static long long _wlel_bsearch_{idx}({t}* a, long long n, {t} key) {{
    long long lo = 0;
    long long hi = n - 1;
    while (lo <= hi) {{
        long long mid = lo + (hi - lo) / 2;
        long long c = {cmp}(a[mid], key);
        if (c == 0) return mid;
        if (c < 0) {{
            lo = mid + 1;
        }} else {{
            hi = mid - 1;
        }}
    }}
    return -1;
}}
")
}

/// The std::fs Result layer: every helper returns `std::Result[.., string]`
/// with the Err payload carrying a C strerror message or a short literal —
/// a file operation can fail, but never panic. The mangled Result enum
/// instances they return are emitted by `emit_types` (the checker registers
/// them when a std::fs call is type-checked), the File wrapper and the
/// arena allocator live in the prelude above.
fn fs_helper_c(name: &str) -> &'static str {
    match name {
        "_wlel_fs_open_r" => {
"static Result__pFile__string _wlel_fs_open_r(const char* path, const char* mode) {
    FILE* h = fopen(path, mode);
    if (!h) {
        return (Result__pFile__string){ ._tag = 1, .v = { .Err = { strerror(errno) } } };
    }
    File* f = (File*)(_wlel_cur_arena ? wlel_arena_alloc(_wlel_cur_arena, sizeof(File)) : malloc(sizeof(File)));
    f->h = (void*)h;
    return (Result__pFile__string){ ._tag = 0, .v = { .Ok = { f } } };
}
"
        }
        "_wlel_fs_read_all_r" => {
"static Result__string__string _wlel_fs_read_all_r(const char* path) {
    FILE* f = fopen(path, \"rb\");
    if (!f) {
        return (Result__string__string){ ._tag = 1, .v = { .Err = { strerror(errno) } } };
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return (Result__string__string){ ._tag = 1, .v = { .Err = { \"cannot seek in file\" } } };
    }
    long n = ftell(f);
    if (n < 0) {
        fclose(f);
        return (Result__string__string){ ._tag = 1, .v = { .Err = { \"cannot determine file size\" } } };
    }
    if (fseek(f, 0, SEEK_SET) != 0) {
        fclose(f);
        return (Result__string__string){ ._tag = 1, .v = { .Err = { \"cannot seek in file\" } } };
    }
    char* buf = _wlel_alloc_str((unsigned long long)n + 1);
    size_t got = n > 0 ? fread(buf, 1, (size_t)n, f) : 0;
    int bad = ferror(f);
    fclose(f);
    if (bad || got != (size_t)n) {
        return (Result__string__string){ ._tag = 1, .v = { .Err = { \"read failed\" } } };
    }
    buf[n] = 0;
    return (Result__string__string){ ._tag = 0, .v = { .Ok = { buf } } };
}
"
        }
        "_wlel_fs_write_all_r" => {
"static Result__int__string _wlel_fs_write_all_r(const char* path, const char* contents) {
    FILE* f = fopen(path, \"wb\");
    if (!f) {
        return (Result__int__string){ ._tag = 1, .v = { .Err = { strerror(errno) } } };
    }
    size_t n = strlen(contents);
    size_t w = fwrite(contents, 1, n, f);
    int bad = ferror(f);
    if (fclose(f) != 0) {
        return (Result__int__string){ ._tag = 1, .v = { .Err = { strerror(errno) } } };
    }
    if (bad || w != n) {
        return (Result__int__string){ ._tag = 1, .v = { .Err = { \"write failed\" } } };
    }
    return (Result__int__string){ ._tag = 0, .v = { .Ok = { (long long)w } } };
}
"
        }
        "_wlel_fs_read_r" => {
"static Result__int__string _wlel_fs_read_r(File* f, unsigned char* buf, long long cap) {
    if (!f || !f->h || cap < 0) {
        return (Result__int__string){ ._tag = 1, .v = { .Err = { \"cannot read from file\" } } };
    }
    size_t n = fread(buf, 1, (size_t)cap, (FILE*)f->h);
    if (n == 0 && ferror((FILE*)f->h)) {
        return (Result__int__string){ ._tag = 1, .v = { .Err = { \"read failed\" } } };
    }
    return (Result__int__string){ ._tag = 0, .v = { .Ok = { (long long)n } } };
}
"
        }
        _ /* "_wlel_fs_write_r" */ => {
"static Result__int__string _wlel_fs_write_r(File* f, const unsigned char* buf, long long n) {
    if (!f || !f->h || n < 0) {
        return (Result__int__string){ ._tag = 1, .v = { .Err = { \"cannot write to file\" } } };
    }
    size_t w = fwrite(buf, 1, (size_t)n, (FILE*)f->h);
    if (w == 0 && ferror((FILE*)f->h)) {
        return (Result__int__string){ ._tag = 1, .v = { .Err = { \"write failed\" } } };
    }
    return (Result__int__string){ ._tag = 0, .v = { .Ok = { (long long)w } } };
}
"
        }
    }
}

/// C wrapper for one `std::format` call signature. `sig` is the kind string
/// the checker built from the argument types ("iis" = int, int, string;
/// "0" = no arguments): each numeric argument is rendered through a stack
/// buffer, strings are inserted verbatim.
fn format_sig_wrapper(sig: &str) -> String {
    let kinds: Vec<char> = if sig == "0" {
        Vec::new()
    } else {
        sig.chars().collect()
    };
    let mut params = String::from("const char* fmt");
    let mut body = String::new();
    if !kinds.is_empty() {
        body.push_str(&format!("    const char* parts[{}];\n", kinds.len()));
    }
    for (k, kind) in kinds.iter().enumerate() {
        match kind {
            'i' => {
                params.push_str(&format!(", long long a{k}"));
                body.push_str(&format!("    char b{k}[32]; snprintf(b{k}, sizeof b{k}, \"%lld\", a{k}); parts[{k}] = b{k};\n"));
            }
            'f' => {
                params.push_str(&format!(", double a{k}"));
                body.push_str(&format!("    char b{k}[64]; snprintf(b{k}, sizeof b{k}, \"%g\", a{k}); parts[{k}] = b{k};\n"));
            }
            'b' => {
                params.push_str(&format!(", _Bool a{k}"));
                body.push_str(&format!("    parts[{k}] = a{k} ? \"true\" : \"false\";\n"));
            }
            _ => {
                // 's': strings are inserted as-is (their own "{}"s are not
                // rescanned)
                params.push_str(&format!(", const char* a{k}"));
                body.push_str(&format!("    parts[{k}] = a{k};\n"));
            }
        }
    }
    let mut out = String::new();
    out.push_str(&format!("static char* _wlel_format_{sig}({}) {{\n", params));
    if kinds.is_empty() {
        out.push_str("    return _wlel_format_join(fmt, 0, 0);\n");
    } else {
        out.push_str(&body);
        out.push_str(&format!(
            "    return _wlel_format_join(fmt, parts, {});\n",
            kinds.len()
        ));
    }
    out.push_str("}\n");
    out
}

/// The concurrency runtime: a thin portable layer over pthreads (POSIX) and
/// the Win32 API (Windows), the `Thread`/`Mutex` handle types Wlel sees, the
/// generic thread spawner (the worker's argument is boxed with malloc so it
/// safely outlives the spawning call), and `sys::sleep_ms`. Spliced in only
/// when the program actually uses `sys::thread/join/mutex_*/chan_*/sleep_ms`.
fn conc_runtime_c() -> String {
    let mut c = String::new();
    c.push_str("/* ---- concurrency runtime (sys::thread, join, mutex, chan, sleep_ms) ---- */\n");
    c.push_str("#ifdef _WIN32\n#include <process.h>\n");
    c.push_str("typedef struct _wlel_thrd { uintptr_t h; int live; } _wlel_thrd;\n");
    c.push_str("typedef unsigned _wlel_thrd_ret;\n");
    c.push_str("#define _WLEL_THRD_CALL __stdcall\n");
    c.push_str("static int _wlel_thrd_spawn(_wlel_thrd* t, unsigned (_WLEL_THRD_CALL* fn)(void*), void* arg) {\n");
    c.push_str("    uintptr_t h = _beginthreadex(0, 0, fn, arg, 0, 0);\n");
    c.push_str("    if (!h) return 0;\n");
    c.push_str("    t->h = h; t->live = 1;\n    return 1;\n}\n");
    c.push_str("static void _wlel_thrd_join(_wlel_thrd* t) {\n");
    c.push_str("    if (t->live) { WaitForSingleObject((HANDLE)t->h, INFINITE); CloseHandle((HANDLE)t->h); t->live = 0; }\n}\n");
    c.push_str("typedef struct _wlel_mtx { CRITICAL_SECTION cs; } _wlel_mtx;\n");
    c.push_str("static void _wlel_mtx_init(_wlel_mtx* m) { InitializeCriticalSection(&m->cs); }\n");
    c.push_str("static void _wlel_mtx_lock(_wlel_mtx* m) { EnterCriticalSection(&m->cs); }\n");
    c.push_str("static void _wlel_mtx_unlock(_wlel_mtx* m) { LeaveCriticalSection(&m->cs); }\n");
    c.push_str("static void _wlel_mtx_destroy(_wlel_mtx* m) { DeleteCriticalSection(&m->cs); }\n");
    c.push_str("typedef struct _wlel_cnd { CONDITION_VARIABLE cv; } _wlel_cnd;\n");
    c.push_str("static void _wlel_cnd_init(_wlel_cnd* c) { InitializeConditionVariable(&c->cv); }\n");
    c.push_str("static void _wlel_cnd_wait(_wlel_cnd* c, _wlel_mtx* m) { SleepConditionVariableCS(&c->cv, &m->cs, INFINITE); }\n");
    c.push_str("static void _wlel_cnd_signal(_wlel_cnd* c) { WakeConditionVariable(&c->cv); }\n");
    c.push_str("static void _wlel_cnd_broadcast(_wlel_cnd* c) { WakeAllConditionVariable(&c->cv); }\n");
    c.push_str("static void _wlel_cnd_destroy(_wlel_cnd* c) { (void)c; }\n");
    c.push_str("static void sys__sleep_ms(long long ms) { if (ms > 0) Sleep((DWORD)ms); }\n");
    c.push_str("#else\n#include <pthread.h>\n");
    c.push_str("typedef struct _wlel_thrd { pthread_t h; int live; } _wlel_thrd;\n");
    c.push_str("typedef void* _wlel_thrd_ret;\n");
    c.push_str("#define _WLEL_THRD_CALL\n");
    c.push_str("static int _wlel_thrd_spawn(_wlel_thrd* t, _wlel_thrd_ret (*fn)(void*), void* arg) {\n");
    c.push_str("    if (pthread_create(&t->h, 0, fn, arg) != 0) return 0;\n");
    c.push_str("    t->live = 1;\n    return 1;\n}\n");
    c.push_str("static void _wlel_thrd_join(_wlel_thrd* t) {\n");
    c.push_str("    if (t->live) { pthread_join(t->h, 0); t->live = 0; }\n}\n");
    c.push_str("typedef struct _wlel_mtx { pthread_mutex_t m; } _wlel_mtx;\n");
    c.push_str("static void _wlel_mtx_init(_wlel_mtx* m) { pthread_mutex_init(&m->m, 0); }\n");
    c.push_str("static void _wlel_mtx_lock(_wlel_mtx* m) { pthread_mutex_lock(&m->m); }\n");
    c.push_str("static void _wlel_mtx_unlock(_wlel_mtx* m) { pthread_mutex_unlock(&m->m); }\n");
    c.push_str("static void _wlel_mtx_destroy(_wlel_mtx* m) { pthread_mutex_destroy(&m->m); }\n");
    c.push_str("typedef struct _wlel_cnd { pthread_cond_t c; } _wlel_cnd;\n");
    c.push_str("static void _wlel_cnd_init(_wlel_cnd* c) { pthread_cond_init(&c->c, 0); }\n");
    c.push_str("static void _wlel_cnd_wait(_wlel_cnd* c, _wlel_mtx* m) { pthread_cond_wait(&c->c, &m->m); }\n");
    c.push_str("static void _wlel_cnd_signal(_wlel_cnd* c) { pthread_cond_signal(&c->c); }\n");
    c.push_str("static void _wlel_cnd_broadcast(_wlel_cnd* c) { pthread_cond_broadcast(&c->c); }\n");
    c.push_str("static void _wlel_cnd_destroy(_wlel_cnd* c) { pthread_cond_destroy(&c->c); }\n");
    c.push_str("static void sys__sleep_ms(long long ms) {\n");
    c.push_str("    struct timespec ts;\n");
    c.push_str("    if (ms < 0) ms = 0;\n");
    c.push_str("    ts.tv_sec = (time_t)(ms / 1000); ts.tv_nsec = (long)(ms % 1000) * 1000000L;\n");
    c.push_str("    nanosleep(&ts, 0);\n}\n");
    c.push_str("#endif\n\n");
    // opaque handle types Wlel declares as reserved: Thread, Mutex
    c.push_str("typedef struct Thread { _wlel_thrd h; } Thread;\n");
    c.push_str("typedef struct Mutex { _wlel_mtx m; } Mutex;\n\n");
    // sys::thread(work, data): the argument is boxed so it outlives the
    // spawning expression; the trampoline (emitted per worker below) frees
    // the box after unpacking
    c.push_str("static Thread* _wlel_thread_spawn(_wlel_thrd_ret (*fn)(void*), const void* arg, unsigned long long size) {\n");
    c.push_str("    void* boxed = malloc(size ? (size_t)size : 1);\n");
    c.push_str("    if (!boxed) return 0;\n");
    c.push_str("    memcpy(boxed, arg, (size_t)size);\n");
    c.push_str("    Thread* t = (Thread*)malloc(sizeof(Thread));\n");
    c.push_str("    if (!t) { free(boxed); return 0; }\n");
    c.push_str("    if (!_wlel_thrd_spawn(&t->h, fn, boxed)) { free(boxed); free(t); return 0; }\n");
    c.push_str("    return t;\n}\n");
    c.push_str("static void sys__join(Thread* t) { if (t) { _wlel_thrd_join(&t->h); free(t); } }\n\n");
    c.push_str("static Mutex* sys__mutex_new(void) {\n");
    c.push_str("    Mutex* m = (Mutex*)malloc(sizeof(Mutex));\n");
    c.push_str("    if (m) _wlel_mtx_init(&m->m);\n    return m;\n}\n");
    c.push_str("static void sys__mutex_lock(Mutex* m) { if (m) _wlel_mtx_lock(&m->m); }\n");
    c.push_str("static void sys__mutex_unlock(Mutex* m) { if (m) _wlel_mtx_unlock(&m->m); }\n");
    c.push_str("static void sys__mutex_free(Mutex* m) { if (m) { _wlel_mtx_destroy(&m->m); free(m); } }\n\n");
    c
}

/// One boxed-argument trampoline per (worker fn, argument type): gives the
/// thread its own root arena, unpacks the argument, runs the worker, then
/// frees the thread's whole arena chain so nothing leaks. Wlel has no
/// function pointers — like the sort comparator, the worker is resolved by
/// name and this helper calls it directly.
fn thread_entry_c(work: &str, ty: &str) -> String {
    let t = c_type(ty);
    format!(
"typedef struct _wlel_tharg_{work} {{ {t} data; }} _wlel_tharg_{work};
static _wlel_thrd_ret _wlel_thread_entry_{work}(void* p) {{
    _wlel_root_arena = wlel_arena_new(65536);
    _wlel_cur_arena = _wlel_root_arena;
    _wlel_tharg_{work}* a = (_wlel_tharg_{work}*)p;
    {work}(a->data);
    free(a);
    wlel_arena_free(_wlel_root_arena);
    _wlel_root_arena = 0;
    _wlel_cur_arena = 0;
    return 0;
}}
")
}

/// The channel runtime for one element type: an unbounded FIFO of heap
/// nodes guarded by a mutex, with a condition variable for blocking
/// receives. `sys::chan_send` never blocks; `sys::chan_recv` blocks until a
/// value arrives or the channel is closed and drained (returns false then,
/// leaving *out untouched); `sys::chan_free` drops queued values.
fn chan_helpers_c(elem: &str) -> String {
    let t = c_type(elem);
    let s = crate::checker::mangle_ty(elem);
    format!(
"typedef struct _wlel_cnode_{s} {{ struct _wlel_cnode_{s}* next; {t} val; }} _wlel_cnode_{s};
struct Chan__{s} {{ _wlel_mtx lock; _wlel_cnd nonempty; _wlel_cnode_{s}* head; _wlel_cnode_{s}** tail; long long count; int closed; }};
static Chan__{s}* _wlel_chan_new_{s}(void) {{
    Chan__{s}* ch = (Chan__{s}*)malloc(sizeof(Chan__{s}));
    if (!ch) return 0;
    _wlel_mtx_init(&ch->lock);
    _wlel_cnd_init(&ch->nonempty);
    ch->head = 0; ch->tail = &ch->head; ch->count = 0; ch->closed = 0;
    return ch;
}}
static _Bool _wlel_chan_send_{s}(Chan__{s}* ch, {t} v) {{
    if (!ch) return 0;
    _wlel_mtx_lock(&ch->lock);
    if (ch->closed) {{ _wlel_mtx_unlock(&ch->lock); return 0; }}
    _wlel_cnode_{s}* n = (_wlel_cnode_{s}*)malloc(sizeof(_wlel_cnode_{s}));
    if (!n) {{ _wlel_mtx_unlock(&ch->lock); return 0; }}
    n->next = 0; n->val = v;
    *ch->tail = n; ch->tail = &n->next; ch->count += 1;
    _wlel_cnd_signal(&ch->nonempty);
    _wlel_mtx_unlock(&ch->lock);
    return 1;
}}
static _Bool _wlel_chan_recv_{s}(Chan__{s}* ch, {t}* out) {{
    if (!ch) return 0;
    _wlel_mtx_lock(&ch->lock);
    while (!ch->head && !ch->closed) _wlel_cnd_wait(&ch->nonempty, &ch->lock);
    if (!ch->head) {{ _wlel_mtx_unlock(&ch->lock); return 0; }}
    _wlel_cnode_{s}* n = ch->head;
    ch->head = n->next;
    if (!ch->head) ch->tail = &ch->head;
    ch->count -= 1;
    _wlel_mtx_unlock(&ch->lock);
    *out = n->val;
    free(n);
    return 1;
}}
static void _wlel_chan_close_{s}(Chan__{s}* ch) {{
    if (!ch) return;
    _wlel_mtx_lock(&ch->lock);
    ch->closed = 1;
    _wlel_cnd_broadcast(&ch->nonempty);
    _wlel_mtx_unlock(&ch->lock);
}}
static void _wlel_chan_free_{s}(Chan__{s}* ch) {{
    if (!ch) return;
    _wlel_mtx_lock(&ch->lock);
    _wlel_cnode_{s}* n = ch->head;
    while (n) {{ _wlel_cnode_{s}* nx = n->next; free(n); n = nx; }}
    ch->head = 0; ch->tail = &ch->head; ch->count = 0;
    _wlel_mtx_unlock(&ch->lock);
    _wlel_cnd_destroy(&ch->nonempty);
    _wlel_mtx_destroy(&ch->lock);
    free(ch);
}}

")
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("    ");
    }
}

/// emit one struct definition
fn emit_struct(cg: &mut Cg, st: &StructDef) {
    cg.out.push_str(&format!("struct {} {{\n", st.name));
    for (fname, fty) in &st.fields {
        cg.out.push_str(&format!("    {} {};\n", c_type(fty), fname));
    }
    cg.out.push_str("};\n\n");
}

/// emit one enum definition: an `int _tag` discriminant plus a union of
/// per-variant payload structs (variants without payloads occupy no union
/// slot; a payload-less enum has no union at all)
fn emit_enum(cg: &mut Cg, e: &EnumDef) {
    cg.out.push_str(&format!("struct {} {{\n", e.name));
    cg.out.push_str("    int _tag;\n");
    let payloadful: Vec<&VariantDef> =
        e.variants.iter().filter(|v| !v.payloads.is_empty()).collect();
    if !payloadful.is_empty() {
        cg.out.push_str("    union {\n");
        for v in &payloadful {
            cg.out.push_str("        struct {\n");
            for (k, pty) in v.payloads.iter().enumerate() {
                cg.out.push_str(&format!("            {} f{};\n", c_type(pty), k));
            }
            cg.out.push_str(&format!("        }} {};\n", v.name));
        }
        cg.out.push_str("    } v;\n");
    }
    cg.out.push_str("};\n\n");
}

/// does the (canonical) type string embed `name` by value? pointers do not
/// count — the forward typedefs cover them
fn ty_embeds(ty: &str, name: &str) -> bool {
    let base = ty.trim_start_matches('*');
    let base = base.split('[').next().unwrap_or(base).trim();
    base == name
}

/// struct + enum definitions in dependency order: an enum must be defined
/// after every struct it embeds by value, a struct after every enum it
/// embeds by value. True cycles (a layout that cannot exist in C) fall
/// back to source order and are reported by cc.
fn emit_types(cg: &mut Cg, p: &Program) {
    let mut sdone = vec![false; p.structs.len()];
    let mut edone = vec![false; p.enums.len()];
    let s_needs_e = |si: usize, ei: usize| -> bool {
        let e = &p.enums[ei];
        p.structs[si]
            .fields
            .iter()
            .any(|(_, t)| ty_embeds(t, &e.name))
    };
    let e_needs_s = |ei: usize, si: usize| -> bool {
        let s = &p.structs[si];
        p.enums[ei]
            .variants
            .iter()
            .any(|v| v.payloads.iter().any(|t| ty_embeds(t, &s.name)))
    };
    let all = |v: &[bool]| v.iter().all(|d| *d);
    while !all(&sdone) || !all(&edone) {
        let mut progress = false;
        for (i, done) in sdone.iter_mut().enumerate() {
            if *done {
                continue;
            }
            if (0..p.enums.len()).all(|j| edone[j] || !s_needs_e(i, j)) {
                emit_struct(cg, &p.structs[i]);
                *done = true;
                progress = true;
            }
        }
        for (i, done) in edone.iter_mut().enumerate() {
            if *done {
                continue;
            }
            if (0..p.structs.len()).all(|j| sdone[j] || !e_needs_s(i, j)) {
                emit_enum(cg, &p.enums[i]);
                *done = true;
                progress = true;
            }
        }
        if !progress {
            for (i, done) in sdone.iter_mut().enumerate() {
                if !*done {
                    emit_struct(cg, &p.structs[i]);
                    *done = true;
                }
            }
            for (i, done) in edone.iter_mut().enumerate() {
                if !*done {
                    emit_enum(cg, &p.enums[i]);
                    *done = true;
                }
            }
        }
    }
}

/// C function name for test #idx: index keeps it unique, the sanitized
/// test name keeps the generated C readable
fn test_c_name(idx: usize, name: &str) -> String {
    let sane: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("_wlel_test_{}_{}", idx, sane)
}

fn gen_test_fn(cg: &mut Cg, idx: usize, t: &TestDef) {
    cg.src_file = t.file.clone();
    cg.sync_line(t.span.start.line);
    cg.out
        .push_str(&format!("static void {}(void) {{\n", test_c_name(idx, &t.name)));
    gen_block(cg, &t.body, 1);
    cg.out.push_str("}\n\n");
}

/// The `wlel test` runner main: each test gets its own setjmp frame, so a
/// failed assert longjmps out of the test body and the suite continues.
fn gen_test_main(cg: &mut Cg, p: &Program) {
    cg.out.push_str("int main(int _wlel_main_argc, char** _wlel_main_argv) {\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("_wlel_binary_stdout();\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("_wlel_is_main = 1;\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("_wlel_argc = _wlel_main_argc; _wlel_argv = _wlel_main_argv;\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("_wlel_root_arena = wlel_arena_new(65536);\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("_wlel_cur_arena = _wlel_root_arena;\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("atexit(__wlel_root_cleanup);\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("long long _wlel_passed = 0;\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("long long _wlel_failed = 0;\n");
    for (i, t) in p.tests.iter().enumerate() {
        let cname = test_c_name(i, &t.name);
        let lit = c_string_literal(&t.name);
        indent(&mut cg.out, 1);
        cg.out.push_str("_wlel_test_active = 1;\n");
        indent(&mut cg.out, 1);
        cg.out.push_str("if (setjmp(_wlel_test_jmp) == 0) {\n");
        indent(&mut cg.out, 2);
        cg.out.push_str(&format!("{}();\n", cname));
        indent(&mut cg.out, 2);
        cg.out.push_str("_wlel_test_active = 0;\n");
        indent(&mut cg.out, 2);
        cg.out.push_str(&format!("printf(\"pass: %s\\n\", {});\n", lit));
        indent(&mut cg.out, 2);
        cg.out.push_str("_wlel_passed += 1;\n");
        indent(&mut cg.out, 1);
        cg.out.push_str("} else {\n");
        indent(&mut cg.out, 2);
        cg.out.push_str("_wlel_test_active = 0;\n");
        indent(&mut cg.out, 2);
        cg.out.push_str(&format!(
            "printf(\"FAIL: %s (%s:%lld)\\n\", {}, _wlel_fail_file, _wlel_fail_line);\n",
            lit
        ));
        indent(&mut cg.out, 2);
        cg.out.push_str("_wlel_failed += 1;\n");
        indent(&mut cg.out, 1);
        cg.out.push_str("}\n");
    }
    indent(&mut cg.out, 1);
    cg.out
        .push_str("printf(\"%lld passed, %lld failed\\n\", _wlel_passed, _wlel_failed);\n");
    indent(&mut cg.out, 1);
    cg.out.push_str("return _wlel_failed > 0 ? 1 : 0;\n");
    cg.out.push_str("}\n");
}

impl Cg {
    /// Emit a `#line` directive so cc diagnostics for the code that follows
    /// point back into the originating `.wl` file (only when the mapped
    /// position actually changes, to keep the C readable).
    fn sync_line(&mut self, line: usize) {
        if self.emit_file != self.src_file || self.emit_line != line {
            let esc = self.src_file.replace('\\', "\\\\").replace('"', "\\\"");
            self.out.push_str(&format!("#line {line} \"{esc}\"\n"));
            self.emit_file = self.src_file.clone();
            self.emit_line = line;
        }
    }
}

fn gen_block(cg: &mut Cg, b: &Block, level: usize) {
    let base = cg.defers.len();
    for s in &b.0 {
        gen_stmt(cg, s, level);
    }
    // scope exit: run this block's defers in reverse (LIFO)
    emit_defers_above(cg, base, level);
}

/// Pop and emit every defer registered above `base` (LIFO) — used at the
/// single fall-through exit of a block, where popping is safe.
fn emit_defers_above(cg: &mut Cg, base: usize, level: usize) {
    while cg.defers.len() > base {
        let item = cg.defers.pop().unwrap();
        emit_defer_item(cg, &item, level);
    }
}

/// Emit defers WITHOUT consuming them — used for return/break/continue:
/// a function or loop body can have several jump sites, and each must run
/// the full defer set.
fn emit_defer_snapshot(cg: &mut Cg, items: &[DeferItem], level: usize) {
    for item in items.iter().rev() {
        emit_defer_item(cg, item, level);
    }
}

fn emit_defer_item(cg: &mut Cg, item: &DeferItem, level: usize) {
    match item {
        DeferItem::Expr(e) => {
            let code = format!("{};\n", gen_expr(cg, e));
            indent(&mut cg.out, level);
            cg.out.push_str(&code);
        }
        DeferItem::Block(stmts) => {
            indent(&mut cg.out, level);
            cg.out.push_str("{\n");
            // a deferred block owns its defer scope: `defer` inside it runs
            // at the end of THIS block (LIFO), not at the enclosing exit —
            // so swap in a fresh stack while generating its statements
            let saved = std::mem::take(&mut cg.defers);
            for s in stmts {
                gen_stmt(cg, s, level + 1);
            }
            while let Some(inner) = cg.defers.pop() {
                emit_defer_item(cg, &inner, level + 1);
            }
            cg.defers = saved;
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
    }
}

/// statement-position `expr?`: evaluate the inner Result/Option once into a
/// temp; on the error variant run the pending defers and return it rebuilt
/// as the enclosing function's error variant, otherwise fall through and
/// hand the temp to the caller (which reads the success payload from it).
/// Returns the temp name.
fn gen_try_propagate(cg: &mut Cg, t: &TryExpr, level: usize) -> String {
    let inner_ty = t.inner.ty.clone().unwrap_or_else(|| "int".into());
    let tmp = format!("_wlel_try{}", cg.tmp_id);
    cg.tmp_id += 1;
    let inner_c = gen_expr(cg, &t.inner);
    indent(&mut cg.out, level);
    cg.out
        .push_str(&format!("{} {} = {};\n", c_type(&inner_ty), tmp, inner_c));
    indent(&mut cg.out, level);
    cg.out
        .push_str(&format!("if ({}._tag == {}) {{\n", tmp, t.err_tag));
    if !cg.defers.is_empty() {
        // the propagate is a function exit: pending defers run first
        let snapshot = cg.defers.clone();
        emit_defer_snapshot(cg, &snapshot, level + 1);
    }
    indent(&mut cg.out, level + 1);
    if t.err_arity > 0 {
        cg.out.push_str(&format!(
            "return ({}){{ ._tag = {}, .v = {{ .{} = {{ {}.v.{}.f0 }} }} }};\n",
            c_type(&t.ret_name),
            t.err_tag,
            t.err_variant,
            tmp,
            t.err_variant
        ));
    } else {
        cg.out.push_str(&format!(
            "return ({}){{ ._tag = {} }};\n",
            c_type(&t.ret_name),
            t.err_tag
        ));
    }
    indent(&mut cg.out, level);
    cg.out.push_str("}\n");
    tmp
}

fn gen_stmt(cg: &mut Cg, s: &Stmt, level: usize) {
    cg.sync_line(s.span.start.line);
    match &s.node {
        StmtKind::Let(name, ty_ann, e) => {
            // try operator: hoisted to a propagate-or-bind sequence — a
            // result temp, the early return on the error variant, then the
            // declaration of the success payload
            if let ExprKind::Try(t) = &e.node {
                let tmp = gen_try_propagate(cg, t, level);
                let decl_ty = ty_ann
                    .clone()
                    .or_else(|| e.ty.clone())
                    .unwrap_or_else(|| "int".into());
                indent(&mut cg.out, level);
                cg.out.push_str(&format!(
                    "{} = {}.v.{}.f0;\n",
                    c_decl(&decl_ty, name),
                    tmp,
                    t.ok_variant
                ));
                return;
            }
            // value-form match: hoisted to a statement sequence — a result
            // temp filled by the arms, then the declaration
            if let ExprKind::Match(ms) = &e.node {
                let decl_ty = ty_ann
                    .clone()
                    .or_else(|| ms.result_ty.clone())
                    .unwrap_or_else(|| "int".into());
                let val = gen_value_match(cg, ms, level);
                indent(&mut cg.out, level);
                cg.out
                    .push_str(&format!("{} = {};\n", c_decl(&decl_ty, name), val));
                return;
            }
            indent(&mut cg.out, level);
            let decl = match ty_ann.as_deref() {
                Some(t) => c_decl(t, name),
                None => format!("{} {}", infer_cty(e), name),
            };
            let init = gen_expr(cg, e);
            cg.out.push_str(&format!("{} = {};\n", decl, init));
        }
        StmtKind::Assign(a) => {
            // try operator on the right-hand side: propagate first, then the
            // plain assignment of the success payload
            if a.op == CompoundOp::Set {
                if let ExprKind::Try(t) = &a.value.node {
                    let tmp = gen_try_propagate(cg, t, level);
                    indent(&mut cg.out, level);
                    let lhs = gen_expr(cg, &a.target);
                    cg.out.push_str(&format!(
                        "{} = {}.v.{}.f0;\n",
                        lhs,
                        tmp,
                        t.ok_variant
                    ));
                    return;
                }
            }
            indent(&mut cg.out, level);
            let op_sym = match a.op {
                CompoundOp::Set => "=",
                CompoundOp::Add => "+=",
                CompoundOp::Sub => "-=",
                CompoundOp::Mul => "*=",
                CompoundOp::Div => "/=",
                CompoundOp::Mod => "%=",
            };
            let lhs = gen_expr(cg, &a.target);
            let rhs = gen_expr(cg, &a.value);
            cg.out.push_str(&format!("{} {} {};\n", lhs, op_sym, rhs));
        }
        StmtKind::If(i) => gen_if(cg, i, level),
        StmtKind::While(cond, body) => {
            indent(&mut cg.out, level);
            let cond_c = gen_expr(cg, cond);
            cg.out.push_str(&format!("while ({}) {{\n", cond_c));
            cg.loop_defer_bases.push(cg.defers.len());
            gen_block(cg, body, level + 1);
            cg.loop_defer_bases.pop();
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
        StmtKind::For(var, start, end, body) => {
            indent(&mut cg.out, level);
            let start_c = gen_expr(cg, start);
            let end_c = gen_expr(cg, end);
            cg.out.push_str(&format!(
                "for (long long {} = {}; {} < {}; {} += 1) {{\n",
                var, start_c, var, end_c, var
            ));
            cg.loop_defer_bases.push(cg.defers.len());
            gen_block(cg, body, level + 1);
            cg.loop_defer_bases.pop();
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
        StmtKind::ForIn(f) => {
            let elem_wl = f.elem.as_deref().unwrap_or("int");
            let elem_c = c_type(elem_wl);
            let iter_c = gen_expr(cg, &f.iter);
            let it = format!("_wlel_it{}", cg.tmp_id);
            let iv = format!("{}_i", it);
            cg.tmp_id += 1;
            indent(&mut cg.out, level);
            // bind the iterated collection once: arrays decay to a pointer
            // (static length), Vec headers are copied (snapshot iteration —
            // pushes during the loop do not extend it; the old buffer stays
            // alive in the arena so reads remain valid)
            let (data_expr, len_expr) = if let Some((_, n)) = array_ty_info(&f.iter.ty) {
                cg.out.push_str(&format!("{}* {} = {};\n", elem_c, it, iter_c));
                (it.clone(), n.to_string())
            } else {
                let vec_ty = f.iter.ty.clone().unwrap_or_else(|| "void".into());
                cg.out.push_str(&format!("{} {} = {};\n", vec_ty, it, iter_c));
                (format!("{}.data", it), format!("{}.len", it))
            };
            cg.out.push_str(&format!(
                "for (long long {} = 0; {} < {}; {} += 1) {{\n",
                iv, iv, len_expr, iv
            ));
            indent(&mut cg.out, level + 1);
            let access = if cg.safe {
                // dev mode: element reads go through the bounds helper —
                // len is re-checked per access, so a shrunken/corrupted
                // collection still reports cleanly instead of UB
                cg.used_arr_at = true;
                format!(
                    "(*({}*)_wlel_arr_at((void*){}, {}, sizeof({}), {}, {}, {}))",
                    elem_c,
                    data_expr,
                    len_expr,
                    elem_c,
                    iv,
                    c_string_literal(&cg.src_file),
                    s.span.start.line
                )
            } else {
                format!("{}[{}]", data_expr, iv)
            };
            cg.out
                .push_str(&format!("{} = {};\n", c_decl(elem_wl, &f.var), access));
            cg.loop_defer_bases.push(cg.defers.len());
            gen_block(cg, &f.body, level + 1);
            cg.loop_defer_bases.pop();
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
        StmtKind::Break => {
            indent(&mut cg.out, level);
            let base = cg.loop_defer_bases.last().copied().unwrap_or(0);
            if cg.defers.len() > base {
                let snapshot: Vec<DeferItem> = cg.defers[base..].to_vec();
                cg.out.push_str("{\n");
                emit_defer_snapshot(cg, &snapshot, level + 1);
                indent(&mut cg.out, level + 1);
                cg.out.push_str("break;\n");
                indent(&mut cg.out, level);
                cg.out.push_str("}\n");
            } else {
                cg.out.push_str("break;\n");
            }
        }
        StmtKind::Continue => {
            indent(&mut cg.out, level);
            let base = cg.loop_defer_bases.last().copied().unwrap_or(0);
            if cg.defers.len() > base {
                let snapshot: Vec<DeferItem> = cg.defers[base..].to_vec();
                cg.out.push_str("{\n");
                emit_defer_snapshot(cg, &snapshot, level + 1);
                indent(&mut cg.out, level + 1);
                cg.out.push_str("continue;\n");
                indent(&mut cg.out, level);
                cg.out.push_str("}\n");
            } else {
                cg.out.push_str("continue;\n");
            }
        }
        StmtKind::Return(e) => {
            indent(&mut cg.out, level);
            match e {
                Some(expr) => {
                    // try operator: propagate the error variant (defer-aware
                    // inside), then defer-aware return of the success payload
                    if let ExprKind::Try(t) = &expr.node {
                        if cg.defers.is_empty() {
                            let tmp = gen_try_propagate(cg, t, level);
                            cg.out
                                .push_str(&format!("return {}.v.{}.f0;\n", tmp, t.ok_variant));
                            return;
                        }
                        let snapshot = cg.defers.clone();
                        cg.out.push_str("{\n");
                        let tmp = gen_try_propagate(cg, t, level + 1);
                        emit_defer_snapshot(cg, &snapshot, level + 1);
                        indent(&mut cg.out, level + 1);
                        cg.out
                            .push_str(&format!("return {}.v.{}.f0;\n", tmp, t.ok_variant));
                        indent(&mut cg.out, level);
                        cg.out.push_str("}\n");
                        return;
                    }
                    // value-form match: arms fill a result temp, then the
                    // usual defer-aware return path takes over
                    if let ExprKind::Match(ms) = &expr.node {
                        let val = gen_value_match(cg, ms, level);
                        if cg.defers.is_empty() {
                            cg.out.push_str(&format!("return {};\n", val));
                        } else {
                            let snapshot = cg.defers.clone();
                            cg.out.push_str("{\n");
                            emit_defer_snapshot(cg, &snapshot, level + 1);
                            indent(&mut cg.out, level + 1);
                            cg.out.push_str(&format!("return {};\n", val));
                            indent(&mut cg.out, level);
                            cg.out.push_str("}\n");
                        }
                        return;
                    }
                    // evaluate the return value BEFORE running defers, so
                    // arena exits can't invalidate memory the value reads
                    let val = gen_expr(cg, expr);
                    if cg.defers.is_empty() {
                        cg.out.push_str(&format!("return {};\n", val));
                    } else {
                        let snapshot = cg.defers.clone();
                        cg.out.push_str("{\n");
                        indent(&mut cg.out, level + 1);
                        cg.out
                            .push_str(&format!("{} _wlel_ret = {};\n", cg.current_ret_c, val));
                        emit_defer_snapshot(cg, &snapshot, level + 1);
                        indent(&mut cg.out, level + 1);
                        cg.out.push_str("return _wlel_ret;\n");
                        indent(&mut cg.out, level);
                        cg.out.push_str("}\n");
                    }
                }
                None => {
                    if cg.defers.is_empty() {
                        if cg.main_void {
                            cg.out.push_str("return 0;\n");
                        } else {
                            cg.out.push_str("return;\n");
                        }
                    } else {
                        // bare return must still run defers (e.g. arena exit)
                        let snapshot = cg.defers.clone();
                        cg.out.push_str("{\n");
                        emit_defer_snapshot(cg, &snapshot, level + 1);
                        indent(&mut cg.out, level + 1);
                        if cg.main_void {
                            cg.out.push_str("return 0;\n");
                        } else {
                            cg.out.push_str("return;\n");
                        }
                        indent(&mut cg.out, level);
                        cg.out.push_str("}\n");
                    }
                }
            }
        }
        StmtKind::ExprStmt(e) => {
            // expr?; — propagate or discard (gen_try_propagate handles the
            // defer-aware error return; nothing to do on the success path)
            if let ExprKind::Try(t) = &e.node {
                gen_try_propagate(cg, t, level);
                return;
            }
            let code = format!("{};\n", gen_expr(cg, e));
            indent(&mut cg.out, level);
            cg.out.push_str(&code);
        }
        StmtKind::Block(b) => {
            indent(&mut cg.out, level);
            cg.out.push_str("{\n");
            gen_block(cg, b, level + 1);
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
        StmtKind::Arena(cap, body) => {
            indent(&mut cg.out, level);
            let cap_expr = match cap {
                Some(c) => gen_expr(cg, c),
                None => "4096".to_string(),
            };
            cg.out.push_str("{\n");
            indent(&mut cg.out, level + 1);
            cg.out.push_str("WArena* _wlel_arena_prev = _wlel_cur_arena;\n");
            indent(&mut cg.out, level + 1);
            cg.out.push_str(&format!(
                "WArena* _wlel_arena_cur = wlel_arena_new({});\n",
                cap_expr
            ));
            indent(&mut cg.out, level + 1);
            cg.out.push_str("_wlel_cur_arena = _wlel_arena_cur;\n");
            // register arena exit as a defer for the arena's inner scope
            // so early returns/breaks inside run it, AND the block exit runs it
            let base = cg.defers.len();
            cg.defers.push(DeferItem::Expr(Spanned::new(
                ExprKind::Call(
                    "__wlel_arena_exit".into(),
                    vec![],
                    vec![Spanned::new(
                        ExprKind::Ident("_wlel_arena_prev".into()),
                        s.span,
                    )],
                ),
                s.span,
            )));
            for s in &body.0 {
                gen_stmt(cg, s, level + 1);
            }
            emit_defers_above(cg, base, level + 1);
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
        StmtKind::Match(ms) => {
            // statement-form match: arms run for effect inside a block that
            // binds the scrutinee to a temp (evaluated exactly once)
            let scrut_c = gen_expr(cg, &ms.scrutinee);
            let scrut_ty = ms
                .scrutinee
                .ty
                .clone()
                .unwrap_or_else(|| "void".into());
            let m = format!("_wlel_m{}", cg.tmp_id);
            cg.tmp_id += 1;
            indent(&mut cg.out, level);
            cg.out.push_str("{\n");
            indent(&mut cg.out, level + 1);
            cg.out
                .push_str(&format!("{} {} = {};\n", c_type(&scrut_ty), m, scrut_c));
            gen_match_arms(cg, ms, &m, None, level + 1);
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
        StmtKind::Defer(DeferBody::Expr(e)) => {
            cg.defers.push(DeferItem::Expr(e.clone()));
        }
        StmtKind::Defer(DeferBody::Block(b)) => {
            cg.defers.push(DeferItem::Block(b.0.clone()));
        }
    }
}

/// value-form match (`let x := match ...` / `return match ...`): declares a
/// result temp, binds the scrutinee to a compiler temp (evaluated once),
/// and lets the arms assign the result; returns the temp's C name.
fn gen_value_match(cg: &mut Cg, ms: &MatchStmt, level: usize) -> String {
    let res_ty = ms.result_ty.clone().unwrap_or_else(|| "int".into());
    let val = format!("_wlel_mv{}", cg.tmp_id);
    cg.tmp_id += 1;
    let m = format!("_wlel_m{}", cg.tmp_id);
    cg.tmp_id += 1;
    let scrut_c = gen_expr(cg, &ms.scrutinee);
    let scrut_ty = ms
        .scrutinee
        .ty
        .clone()
        .unwrap_or_else(|| "void".into());
    indent(&mut cg.out, level);
    cg.out.push_str(&format!("{} {};\n", c_type(&res_ty), val));
    indent(&mut cg.out, level);
    cg.out.push_str("{\n");
    indent(&mut cg.out, level + 1);
    cg.out
        .push_str(&format!("{} {} = {};\n", c_type(&scrut_ty), m, scrut_c));
    gen_match_arms(cg, ms, &m, Some(&val), level + 1);
    indent(&mut cg.out, level);
    cg.out.push_str("}\n");
    val
}

/// the if/else chain of a match: each arm checks the tag, declares its
/// payload bindings (fresh scope, mirroring the checker), and runs the body.
/// Exhaustiveness (proven by the checker) guarantees the value target is
/// assigned on every path that reaches the code after the match.
fn gen_match_arms(cg: &mut Cg, ms: &MatchStmt, scrut: &str, target: Option<&str>, level: usize) {
    for (i, arm) in ms.arms.iter().enumerate() {
        cg.sync_line(arm.span.start.line);
        indent(&mut cg.out, level);
        let prefix = if i == 0 {
            match &arm.pattern {
                Pattern::Wildcard => "if (1) {\n".to_string(),
                Pattern::Variant { tag, .. } => {
                    format!("if ({}._tag == {}) {{\n", scrut, tag)
                }
            }
        } else {
            match &arm.pattern {
                Pattern::Wildcard => "} else {\n".to_string(),
                Pattern::Variant { tag, .. } => {
                    format!("}} else if ({}._tag == {}) {{\n", scrut, tag)
                }
            }
        };
        cg.out.push_str(&prefix);
        if let Pattern::Variant { name: vname, binds, .. } = &arm.pattern {
            for (k, b) in binds.iter().enumerate() {
                if b.name == "_" {
                    continue; // discard: no declaration, no read
                }
                let bty = b.ty.as_deref().unwrap_or("int");
                indent(&mut cg.out, level + 1);
                cg.out.push_str(&format!(
                    "{} = {}.v.{}.f{};\n",
                    c_decl(bty, &b.name),
                    scrut,
                    vname,
                    k
                ));
            }
        }
        match &arm.body {
            MatchBody::Block(b) => {
                gen_block(cg, b, level + 1);
            }
            MatchBody::Expr(x) => {
                let code = match target {
                    Some(t) => format!("{} = {};\n", t, gen_expr(cg, x)),
                    None => format!("{};\n", gen_expr(cg, x)),
                };
                indent(&mut cg.out, level + 1);
                cg.out.push_str(&code);
            }
        }
    }
    if !ms.arms.is_empty() {
        indent(&mut cg.out, level);
        cg.out.push_str("}\n");
    }
}

fn gen_if(cg: &mut Cg, i: &IfStmt, level: usize) {
    let cond_c = gen_expr(cg, &i.cond);
    indent(&mut cg.out, level);
    cg.out.push_str(&format!("if ({}) {{\n", cond_c));
    gen_block(cg, &i.then_body, level + 1);
    match &i.else_branch {
        None => {
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
        Some(ElseBranch::Block(els)) => {
            indent(&mut cg.out, level);
            cg.out.push_str("} else {\n");
            gen_block(cg, els, level + 1);
            indent(&mut cg.out, level);
            cg.out.push_str("}\n");
        }
        Some(ElseBranch::If(inner)) => {
            indent(&mut cg.out, level);
            cg.out.push_str("} else ");
            gen_if(cg, inner, level);
        }
    }
}

fn gen_expr(cg: &mut Cg, e: &Expr) -> String {
    match &e.node {
        ExprKind::Int(v) => v.to_string(),
        ExprKind::UInt(v) => format!("{v}ULL"),
        ExprKind::Float(v) => format!("{v:?}"),
        ExprKind::Str(s) => c_string_literal(s),
        ExprKind::Bool(true) => "1".into(),
        ExprKind::Bool(false) => "0".into(),
        ExprKind::Ident(n) => n.clone(),
        ExprKind::Unary(UnOp::Neg, x) => format!("(-{})", gen_expr(cg, x)),
        ExprKind::Unary(UnOp::Not, x) => format!("(!{})", gen_expr(cg, x)),
        ExprKind::Unary(UnOp::BitNot, x) => format!("(~{})", gen_expr(cg, x)),
        ExprKind::Binary(op, l, r) => {
            // safe mode: integer / and % abort on a zero divisor, reporting
            // the .wl position; floats keep IEEE inf semantics everywhere
            if cg.safe
                && matches!(op, BinOp::Div | BinOp::Mod)
                && is_int_ty(&l.ty)
                && is_int_ty(&r.ty)
            {
                cg.used_divz = true;
                return format!(
                    "({} {} _wlel_divz({}, {}, {}))",
                    gen_expr(cg, l),
                    binop_str(*op),
                    gen_expr(cg, r),
                    c_string_literal(&cg.src_file),
                    e.span.start.line
                );
            }
            format!(
                "({} {} {})",
                gen_expr(cg, l),
                binop_str(*op),
                gen_expr(cg, r)
            )
        }
        ExprKind::Call(f, _ty_args, args) => {
            if f == "std::len" {
                let arr = gen_expr(cg, &args[0]);
                return format!("((long long)(sizeof({}) / sizeof(({}[0]))))", arr, arr);
            }
            if f == "arena_stats" {
                return "_wlel_arena_stats()".into();
            }
            // std::sort / std::binary_search were rewritten by the checker:
            // trailing metadata args carry the element type and comparator
            // name; one specialized C helper per (element, comparator) pair
            // is spliced in after the prototypes, only for pairs in use
            if f == "_wlel_sort" || f == "_wlel_bsearch" {
                let meta = |a: &Expr| match &a.node {
                    ExprKind::Str(s) => s.clone(),
                    _ => String::new(),
                };
                let elem = meta(&args[args.len() - 2]);
                let cmp = meta(args.last().expect("sort metadata"));
                let searching = f == "_wlel_bsearch";
                let idx = {
                    let reg = if searching {
                        &mut cg.bsearches
                    } else {
                        &mut cg.sorts
                    };
                    match reg.iter().position(|(e, c)| *e == elem && *c == cmp) {
                        Some(i) => i,
                        None => {
                            reg.push((elem, cmp));
                            reg.len() - 1
                        }
                    }
                };
                let name = if searching {
                    format!("_wlel_bsearch_{idx}")
                } else {
                    format!("_wlel_sort_{idx}")
                };
                let mut parts: Vec<String> = Vec::new();
                for a in &args[..args.len() - 2] {
                    parts.push(gen_expr(cg, a));
                }
                if searching {
                    return format!("{name}({})", parts.join(", "));
                }
                // (base, len) -> quicksort over [0, len)
                let len_expr = parts.pop().unwrap_or_else(|| "0".into());
                let base_expr = parts.pop().unwrap_or_else(|| "0".into());
                return format!("{name}({base_expr}, 0, ({len_expr}) - 1)");
            }
            // std::format was rewritten by the checker to a per-signature
            // helper; remember the signature so its wrapper gets spliced in
            if let Some(sig) = f.strip_prefix("_wlel_format_") {
                let sig = sig.to_string();
                if !cg.format_sigs.contains(&sig) {
                    cg.format_sigs.push(sig);
                }
            }
            // std::fs Result helpers are spliced in only when called
            if f.starts_with("_wlel_fs_") && f.ends_with("_r") && !cg.fs_helpers.contains(&f.as_str()) {
                let static_name: &'static str = match f.as_str() {
                    "_wlel_fs_open_r" => "_wlel_fs_open_r",
                    "_wlel_fs_read_all_r" => "_wlel_fs_read_all_r",
                    "_wlel_fs_write_all_r" => "_wlel_fs_write_all_r",
                    "_wlel_fs_read_r" => "_wlel_fs_read_r",
                    "_wlel_fs_write_r" => "_wlel_fs_write_r",
                    _ => unreachable!("unknown fs helper"),
                };
                cg.fs_helpers.push(static_name);
            }
            // sys::thread was rewritten by the checker: metadata carries the
            // worker name and its (single) parameter type; one boxed-arg
            // trampoline per worker is spliced in after the prototypes
            if f == "_wlel_thread_spawn" {
                let meta = |a: &Expr| match &a.node {
                    ExprKind::Str(s) => s.clone(),
                    _ => String::new(),
                };
                cg.used_conc = true;
                let work = meta(&args[0]);
                let tyname = meta(&args[1]);
                if !cg.threads.iter().any(|(w, t)| w == &work && t == &tyname) {
                    cg.threads.push((work.clone(), tyname));
                }
                let data = gen_expr(cg, &args[2]);
                return format!(
                    "_wlel_thread_spawn(_wlel_thread_entry_{work}, (const void*)&(_wlel_tharg_{work}){{ .data = {data} }}, sizeof(_wlel_tharg_{work}))"
                );
            }
            // sys::chan_* were rewritten by the checker with the element type
            // as trailing metadata; one runtime struct + function set per
            // element type is spliced in with the concurrency layer
            if f == "_wlel_chan_new"
                || f == "_wlel_chan_send"
                || f == "_wlel_chan_recv"
                || f == "_wlel_chan_close"
                || f == "_wlel_chan_free"
            {
                cg.used_conc = true;
                let elem = match &args[args.len() - 1].node {
                    ExprKind::Str(s) => s.clone(),
                    _ => String::new(),
                };
                if !cg.chans.contains(&elem) {
                    cg.chans.push(elem.clone());
                }
                let s = crate::checker::mangle_ty(&elem);
                let name = format!("{f}_{s}");
                let real: Vec<String> = args[..args.len() - 1]
                    .iter()
                    .map(|x| gen_expr(cg, x))
                    .collect();
                return format!("{name}({})", real.join(", "));
            }
            // the plain sys:: wrappers live in the concurrency runtime too
            if f == "sys::join" || f == "sys::mutex_new" || f == "sys::mutex_lock"
                || f == "sys::mutex_unlock" || f == "sys::mutex_free" || f == "sys::sleep_ms"
            {
                cg.used_conc = true;
            }
            let a: Vec<String> = args.iter().map(|x| gen_expr(cg, x)).collect();
            format!("{}({})", f.replace("::", "__"), a.join(", "))
        }
        ExprKind::Sizeof(ty) => format!("sizeof({})", c_sizeof_ty(ty)),
        ExprKind::New(ty, count) => {
            let elem_ty = c_type(ty);
            let sz = c_sizeof_ty(ty);
            match count {
                Some(cnt) => {
                    let c_expr = gen_expr(cg, cnt);
                    format!(
                        "({}*)(_wlel_cur_arena ? wlel_arena_alloc(_wlel_cur_arena, sizeof({}) * ({})) : malloc(sizeof({}) * ({})))",
                        elem_ty, sz, c_expr, sz, c_expr
                    )
                }
                None => {
                    format!(
                        "({}*)(_wlel_cur_arena ? wlel_arena_alloc(_wlel_cur_arena, sizeof({})) : malloc(sizeof({})))",
                        elem_ty, sz, sz
                    )
                }
            }
        }
        ExprKind::CurrentArena => "(void*)_wlel_cur_arena".into(),
        ExprKind::Cast(ty, x) => format!("({})({})", c_type(ty), gen_expr(cg, x)),
        ExprKind::Index(b, i) => {
            let base_c = gen_expr(cg, b);
            let idx_c = gen_expr(cg, i);
            // safe mode: arrays carry their length in the checker-annotated
            // type ("[T; N]") — route the access through a checked helper.
            // Pointers (new(T, n)) have no tracked length and stay unchecked.
            if cg.safe {
                if let Some((elem, n)) = array_ty_info(&b.ty) {
                    let elem_c = c_type(&elem);
                    cg.used_arr_at = true;
                    return format!(
                        "(*({}*)_wlel_arr_at({}, {}, sizeof({}), {}, {}, {}))",
                        elem_c,
                        base_c,
                        n,
                        elem_c,
                        idx_c,
                        c_string_literal(&cg.src_file),
                        e.span.start.line
                    );
                }
            }
            format!("{}[{}]", base_c, idx_c)
        }
        ExprKind::ArrayLit(elems) => {
            let parts: Vec<String> = elems.iter().map(|x| gen_expr(cg, x)).collect();
            format!("{{ {} }}", parts.join(", "))
        }
        ExprKind::AddrOf(x) => format!("&{}", gen_expr(cg, x)),
        ExprKind::Deref(x) => format!("(*{})", gen_expr(cg, x)),
        ExprKind::Field(obj, f) => format!("{}.{}", gen_expr(cg, obj), f),
        ExprKind::StructLit(name, fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(n, v)| format!(".{} = {}", n, gen_expr(cg, v)))
                .collect();
            format!("({}){{ {} }}", name, parts.join(", "))
        }
        ExprKind::EnumLit(ename, tag, vname, payloads) => {
            // C99 compound literal of the enum's tag+union struct
            if payloads.is_empty() {
                format!("({}){{ ._tag = {} }}", ename, tag)
            } else {
                let fields: Vec<String> = payloads
                    .iter()
                    .enumerate()
                    .map(|(k, p)| format!(".f{} = {}", k, gen_expr(cg, p)))
                    .collect();
                format!(
                    "({}){{ ._tag = {}, .v = {{ .{} = {{ {} }} }} }}",
                    ename,
                    tag,
                    vname,
                    fields.join(", ")
                )
            }
        }
        // the checker rejects a match outside the hoisted statement
        // positions, so codegen never meets one here
        ExprKind::Match(_) => unreachable!("match expression outside let/return/statement"),
        // the checker rejects a try operator outside the hoisted statement
        // positions, so codegen never meets one here either
        ExprKind::Try(_) => unreachable!("try operator outside let/assign/return/statement"),
        // the checker rewrites every method call into a plain call to the
        // impl's desugared function before codegen sees it
        ExprKind::MethodCall(..) => unreachable!("method call survived type checking"),
    }
}

fn binop_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::Le => "<=",
        BinOp::Ge => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

/// is this checker-annotated type name an integer type? (canonical names
/// only — `Type::name()` maps byte/char to "u8" and i64 to "int")
fn is_int_ty(t: &Option<String>) -> bool {
    matches!(
        t.as_deref(),
        Some("int" | "i8" | "i16" | "i32" | "u8" | "u16" | "u32" | "u64" | "usize")
    )
}

/// element type + length of an annotated array type ("[int; 3]" ->
/// ("int", 3)). Nested arrays are declined: their element cast would need
/// C pointer-to-array types, so callers fall back to unchecked indexing.
fn array_ty_info(ty: &Option<String>) -> Option<(String, usize)> {
    let t = ty.as_deref()?;
    let rest = t.strip_prefix('[')?;
    let close = rest.rfind(']')?;
    let inner = &rest[..close];
    let (base, n) = inner.rsplit_once(';')?;
    let n: usize = n.trim().parse().ok()?;
    let base = base.trim().to_string();
    if base.starts_with('[') {
        return None;
    }
    Some((base, n))
}

/// exact C type for a resolved Wlel type string ("int", "*Pt", "u8", ...)
fn c_type(t: &str) -> String {    let stars = t.chars().take_while(|c| *c == '*').count();
    let base = &t[stars..];
    let base_c = match base {
        "int" | "i64" => "long long",
        "i8" => "int8_t",
        "i16" => "int16_t",
        "i32" => "int32_t",
        "u8" | "byte" | "char" => "uint8_t",
        "u16" => "uint16_t",
        "u32" => "uint32_t",
        "u64" => "uint64_t",
        "usize" => "size_t",
        "float" | "f64" => "double",
        "f32" => "float",
        "bool" => "_Bool",
        "string" => "const char*",
        "void" => "void",
        other => other, // struct name
    };
    format!("{}{}", base_c, "*".repeat(stars))
}

/// type inside sizeof(): arrays keep their length (`long long[5]`)
fn c_sizeof_ty(ty: &str) -> String {
    if let Some(rest) = ty.strip_prefix('[') {
        let close = rest.rfind(']').unwrap_or(rest.len());
        let inner = &rest[..close];
        if let Some((base, n)) = inner.rsplit_once(';') {
            return format!("{}[{}]", c_type(base.trim()), n.trim());
        }
    }
    c_type(ty)
}

/// declaration fragment: arrays need `long long a[4]`, others `long long a`
fn c_decl(ty: &str, name: &str) -> String {
    if let Some(rest) = ty.strip_prefix('[') {
        let close = rest.rfind(']').unwrap_or(rest.len());
        let inner = &rest[..close];
        if let Some((base, n)) = inner.rsplit_once(';') {
            let bt = c_type(base.trim());
            let stars = bt.chars().take_while(|c| *c == '*').count();
            return format!("{}{} {}[{}]", &bt[stars..], "*".repeat(stars), name, n.trim());
        }
    }
    format!("{} {}", c_type(ty), name)
}

/// fallback when the checker did not annotate (direct gen without check)
fn infer_cty(e: &Expr) -> String {
    match &e.node {
        ExprKind::Float(_) => "double".into(),
        ExprKind::Str(_) => "const char*".into(),
        ExprKind::Bool(_) => "_Bool".into(),
        // suffixed literal: `x := 10u8` without a checker annotation
        ExprKind::Cast(ty, _) => c_type(ty),
        _ => "long long".into(),
    }
}

fn c_string_literal(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
