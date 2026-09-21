# Wlel

[![CI](https://github.com/WildanDeveloper/wlel/actions/workflows/ci.yml/badge.svg)](https://github.com/WildanDeveloper/wlel/actions/workflows/ci.yml)
[![fuzz](https://github.com/WildanDeveloper/wlel/actions/workflows/fuzz.yml/badge.svg)](https://github.com/WildanDeveloper/wlel/actions/workflows/fuzz.yml)

**The Arena-First Systems Programming Language**

Simple enough to learn in one afternoon. Fast as C. Zero leaks by construction.

```wl
use std;

struct Node { val: int, next: *Node }

fn main() -> int {
    // 1 MiB bump arena: every allocation inside is an O(1) pointer bump
    arena(1 << 20) {
        let head: *Node = 0 as *Node;
        i := 0;
        while i < 100_000 {
            n := new(Node);   // typed, arena-aware
            n.val = i;
            n.next = head;
            head = n;
            i += 1;
        }
        std::println_int(head.val);
    } // <- all 100k nodes freed together in O(1). Zero leak, zero GC pause.

    return 0;
}
```

Wlel transpiles to clean, human-readable C99 and compiles with `cc`/`gcc`/`clang`
at `-O2`/`-O3`. There is no VM, no GC, and no runtime above C — benchmarks are at
parity with hand-written C (`fib(35)`: 12 ms vs C's 11 ms). What you get on top
is a modern type checker, first-class arenas as language syntax, deterministic
`defer`, and tooling that ships in a single binary.

---

## Why is this unique?

Every language "has arenas". Wlel is the only one where **the arena is the syntax**:

| | How you get memory safety | The catch |
|---|---|---|
| **C/C++** | Discipline + third-party arena libraries | Manual `free`, leaks, use-after-free |
| **Rust / Zig** | Arena as a library pattern (`ArenaAllocator`) | You have to remember to use it |
| **Go / Java** | Garbage collector | GC pauses, 2–3x memory overhead |
| **Wlel** | `arena(bytes) { ... }` + `new(T)` — **in the language itself** | — |

Inside an `arena` block you never write `free`. Early `return`, `break`, and
`defer` are safe by construction: return values are evaluated *before* the arena
is freed (LIFO unwind — verified with AddressSanitizer). Strings you build with
`+` live in a root arena that is released automatically at exit, so **Wlel
programs pass LeakSanitizer by default** — something C and C++ cannot claim.

## Feature status (pre-1.0)

### Language
- [x] First-class arenas: `arena(size) { ... }`, `arena { ... }` (default 4 KiB), `wlel_arena()`
- [x] Typed allocator: `new(T)`, `new(T, count)` — arena-aware, falls back to heap outside arenas
- [x] Root arena: temporaries and global allocations freed at exit via `atexit` — zero leaks end-to-end
- [x] Safe unwind: early return / `break` / `defer` inside arenas evaluate the return value first
- [x] `defer` — LIFO at scope exit, loop-aware, early-return-aware; multi-statement `defer { ... }` blocks with their own defer scope
- [x] Control flow: `if / else if / else`, `while`, `for i in a..b`, `break`, `continue`
- [x] Structs (nominal, C layout), struct literals, field access with auto-deref; forward & recursive definitions
- [x] Pointers (`*T`, `&`, `*`), arrays `[T; N]` with C-style decay, `wlel_sizeof(T)`
- [x] Casts: `expr as T` (numerics, pointers, int↔ptr)
- [x] Compound assignment `+= -= *= /= %=`, bitwise `& | ^ ~ << >>`
- [x] Modules: recursive `use "path.wl";` with diamond-import deduplication, `use std;`
- [x] FFI: `extern fn sqrt(x: f64) -> f64;` binds straight to C symbols; structs are C structs (see [FFI](#ffi))
- [x] Reserved identifier prefix `_wlel_` protected by the compiler

### Numbers
- [x] Width types: `u8 u16 u32 u64, i8 i16 i32 i64, usize, f32 f64` (`int` = i64, `float` = f64, `byte`/`char` = u8)
- [x] Literal suffixes: `10u8`, `2.5f32`, full u64 range (`18_446_744_073_709_551_615u64`)
- [x] Untyped literals adapt to context with range checks; narrowing requires an explicit cast; mixed widths require a cast
- [x] Overflow is never UB — see [Overflow semantics](#overflow-semantics)

### Diagnostics & tooling
- [x] Source spans everywhere: errors point at `file.wl:12:5`; `#line` directives map cc errors back to `.wl`
- [x] Parser error recovery: one compile reports *all* syntax errors, not just the first
- [x] Warning system: unused variables/parameters/imports, unreachable code — non-fatal, `_`-prefix opt-out
- [x] Built-in tests: `test "name" { ... }` blocks + `wlel test` runner with `file:line` failures
- [x] Safe-debug mode: `wlel run`/`wlel test` compile with array bounds checks, integer div-zero checks and arena poison-fill; `wlel build` stays zero-overhead (see [Safe-debug vs release](#safe-debug-vs-release))
- [x] Arena stats: `arena_stats() -> ArenaStats { bytes, chunks, peak }` for the innermost active arena (root when none)
- [x] `wlel build -sanitize` — ASan-instrumented release binary
- [x] CLI: `wlel run` (content-cached binaries), `wlel build -o out [-O0..-O3] [-sanitize] [--emit-c] [-l <lib>] [-L <dir>]`, `wlel check`, `wlel test`
- [x] Lexer: hex/bin/oct/scientific/`_`-separated literals, strings with escapes + UTF-8
- [x] Type checker: inference, annotations, return-path analysis, loop/arena depth tracking
- [x] Generics: `fn id[T](x: T) -> T` and `struct Box[T] { val: T }` via checker-level monomorphization — type inference at call sites, per-type instantiation (mangled C like `Box__int`), no boxing, no vtables (see [Generics](#generics))
- [x] Stdlib: `std::print*`/`println*`, `strlen`, `streq`, `abs`, `min`, `max`, `len`, `checked_add/sub/mul`, `format`, `parse_float`, `float_to_str`; collections `Vec[T]`, `HashMap[K,V]` + `for x in coll`; string library `str_find/sub/trim/split/parse_int`, `int_to_str`, `str_cmp`; sort & search `std::sort`, `std::binary_search`; file I/O `sys::read_file/write_file`, `std::fs::open/read/write/close`; `sys::argc/arg/exit`
- [x] Projects: `wlel new`, `wlel.toml` manifests, path + git dependencies (auto-cloned, revision-pinned by `wlel.lock`), nested deps, `wlel run/build/check/test` in project mode (see [Projects](#projects))
- [x] Formatter: `wlel fmt` — canonical, idempotent, comment-preserving; `wlel fmt` in a project formats every `.wl`; `-check` mode for CI (see [Formatting](#formatting))

### Not yet (planned, in order)
- [ ] `wlel doc`
- [ ] Tagged enums + `match`, `Result[T,E]`, method sugar, LSP, concurrency
- [ ] Direct QBE backend (drops the C-compiler dependency), self-hosting

## Try it

Requires a C compiler (`cc`, `gcc`, or `clang`) and Rust/Cargo to build `wlel` itself.

```bash
cargo run --release -- run examples/arena.wl             # scoped arena demo
cargo run --release -- run examples/loops.wl             # for-in range demo
cargo run --release -- run examples/strings.wl           # string concat/eq demo
cargo run --release -- run examples/args.wl -- greet you # CLI args demo
cargo run --release -- run examples/fileio.wl -- examples/fileio.wl # cat demo (file I/O)
cargo run --release -- run examples/generics.wl          # generic fn/struct demo
cargo run --release -- run examples/sort.wl              # sort & search demo + 1M int benchmark
cargo run --release -- run examples/overflow.wl          # wrap + checked ops demo
cargo run --release -- run examples/fib.wl               # fibonacci benchmark
cargo run --release -- test examples/strings.wl          # run the test blocks in a file
cargo run --release -- test examples/kitchen.wl          # tests from imports run too
cargo run --release -- check examples/kitchen.wl         # type-check only
cargo run --release -- build examples/arena.wl -o arena -O3 --emit-c
```

Project mode (needs `wlel` on PATH):

```bash
wlel new myapp && cd myapp   # scaffold: wlel.toml + src/main.wl
wlel run                     # run the project (dev checks)
wlel test                    # project + dependency tests
wlel build                   # binary named after the package
wlel fmt                     # canonical formatting for every .wl
```

## A quick tour

Declarations, functions, control flow:

```wl
fn fib(n: int) -> int {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> int {
    x := 41;              // inferred
    let y: int = x + 1;   // annotated
    for i in 0..3 { std::println_int(i); }
    return fib(10);
}
```

`defer` runs at scope exit — LIFO, and it runs on early returns and loop jumps:

```wl
fn process(path: string) -> int {
    defer std::println_str("done");
    defer {
        std::println_str("cleanup step 1");
        std::println_str("cleanup step 2");
    }
    if path == "" { return 1; }   // defers still run
    return 0;
}
```

Width types make data layouts exact; narrowing is always explicit:

```wl
let small: u8 = 250u8;      // suffix literal
let wrapped: u8 = small + 10;   // wraps to 4 (mod 256), by design
let big: u64 = 18_000_000_000_000_000_000u64;
let risky: u8 = small as u8;    // explicit cast required for narrowing
```

## Generics

Functions and structs can be generic over types; each use is monomorphized to a
concrete instance at compile time — zero runtime cost:

```wl
struct Box[T] {
    val: T,
}

fn box_get[T](b: Box[T]) -> T {
    return b.val;
}

fn main() -> int {
    let b: Box[float] = Box { val: 2.5 };
    std::println_float(box_get(b));   // 2.5
    return 0;
}
```

Type arguments are usually inferred from the call (`first(1, 2)` needs no
`[int]`); explicit ones work too. Every concrete instantiation is emitted under
a mangled name (`Box__int`, `Box__Box__int`), so nested generics and generics
inside arenas/safe-mode compose naturally. A generic that is never called is
never monomorphized — its body is not even checked.

## Overflow semantics

Integer arithmetic in Wlel **never panics and is never undefined** — results are
fully defined in debug and release alike:

| Case | Behavior |
|------|----------|
| Unsigned (`u8..u64`, `usize`) | wrap modulo 2^N (standard C) |
| Signed (`i8..i64`) | two's-complement wrap — the backend compiles with `-fwrapv` |
| Widths below 32 bits | the compiler inserts explicit wrap casts, so wrapping happens at the original width, not in C's `int` |

When wrapping is *not* what you want, the stdlib detects overflow — no
exceptions, no panics:

```wl
use std;

fn main() -> int {
    r := 0;
    if std::checked_add(9_000_000_000, 9_000_000_000, &r) {
        std::println_int(r);          // safe: r is valid
    } else {
        std::println_str("overflow"); // r holds the wrapped value — don't use it
    }
    return 0;
}
```

`std::checked_add/sub/mul(a: int, b: int, out: *int) -> bool` returns `false` on
overflow. Note the builtins still store the wrapped result in `*out` even when
they return `false` — treat `*out` as valid only when the call returned `true`.

## File I/O

Two layers, both arena-aware. **Whole-file helpers** for the common case:

```wl
use std;

fn main() -> int {
    content := "";
    if !sys::read_file("notes.txt", &content) {   // false on any failure
        std::println_str("cannot read notes.txt");
        return 1;
    }
    std::print_str(content);   // a cat in four lines
    return 0;
}
```

**Streaming handles** for buffers and binary data — `close` is idempotent and
defer-friendly, handles die with their arena, so LeakSanitizer stays clean:

```wl
f := std::fs::open("data.bin", "rb");
if f == 0 as *File { return 1; }        // null on failure
defer std::fs::close(f);
buf := new(u8, 4096);
while true {
    n := std::fs::read(f, buf, 4096);   // bytes read; 0 = EOF, -1 = error
    if n <= 0 { break; }
    // ... process buf[0..n) ...
}
```

- `sys::read_file(path, &out) -> bool` / `sys::write_file(path, contents) -> bool` — no import required
- `std::fs::open(path, mode) -> *File`, `fs::read(f, buf: *u8, n) -> int`, `fs::write(f, buf: *u8, n) -> int`, `fs::close(f)` — `use std;` required
- Strings are NUL-terminated: after a partial read, set `buf[n] = 0` before
  treating the slice as a string; true binary data stays in `*u8` buffers

## Sort & search

`std::sort` and `std::binary_search` take an ordinary Wlel function as the
comparator — `cmp(a, b) -> int`, negative / zero / positive like `strcmp`.
No function pointers, no `qsort` `void*`: the compiler emits a specialized
C sort per (element, comparator) pair, so the comparator call inlines at
`-O2` (1M random ints sort in ~77 ms — faster than C `qsort` at ~111 ms on
the same machine, which pays for indirect calls):

```wl
use std;

fn asc(a: int, b: int) -> int {
    if a < b { return -1; }
    if a > b { return 1; }
    return 0;
}

fn main() -> int {
    a := [5, 3, 9, 1];
    std::sort(a, asc);                          // fixed array [T; N]
    v := vec_new[string]();
    vec_push(&v, "pear");
    vec_push(&v, "apple");
    std::sort(v, str_cmp);                      // Vec[T], stdlib comparator
    i := std::binary_search(a, 9, asc);         // index of a match, or -1
    return 0;
}
```

Three collection forms work for both functions: `(arr, cmp)`,
`(ptr, n, cmp)` and `(vec, cmp)` — `binary_search` takes the needle before
the comparator. Sorting is in-place, not stable (like C `qsort`); any
element type works, structs included — write a comparator over a field.

## Testing

Tests live next to the code they verify, in plain `.wl` files:

```wl
fn add(a: int, b: int) -> int {
    return a + b;
}

test "basic arithmetic" {
    assert_eq(add(1, 1), 2);
    assert_eq("he" + "llo", "hello");
}

test "string" {
    assert("arena" != "borrow");
}
```

```bash
$ wlel test math.wl
pass: basic arithmetic
pass: string
2 passed, 0 failed
```

- **`assert(cond)`** and **`assert_eq(a, b)`** are builtins usable in tests and
  ordinary code. `assert_eq` uses the same comparison semantics as `==`
  (literal width adaptation, explicit casts for narrowing).
- A failed assert reports the **source location**: `FAIL: kaput (main.wl:5)`.
- One failing test does not stop the suite — the runner catches it and
  continues; the exit code is non-zero if anything failed.
- `wlel test file.wl` also runs tests from **imported files** (`use "lib/x.wl"`),
  so libraries ship their own tests. `wlel run`/`wlel build` ignore test blocks
  entirely — zero overhead in the shipped binary.

## FFI

`extern fn` declares a C function; the call compiles to a direct C call and
the linker resolves the symbol.

```wl
use std;

// double sqrt(double) — libm is linked by default
extern fn sqrt(x: f64) -> f64;

// libc: int puts(const char*), int atoi(const char*)
extern fn puts(s: string) -> i32;
extern fn atoi(s: string) -> i32;

fn main() -> int {
    puts("sqrt(2) is about");
    std::println_int(atoi("3") + 1);   // 4
    std::println_float(sqrt(2.0));     // 1.41421...
    return 0;
}
```

- **Declare the exact C types.** Wlel `int` is i64 (C `long long`), so C
  `int` is `i32`, C `size_t` is `usize`, C `double` is `f64`. A mismatched
  declaration is caught by cc as a conflicting-types error.
- **Wlel structs are C structs** — same fields, same order, same layout
  (locked by `static_assert`/`offsetof` tests). Third-party bindings
  (raylib, SDL) name their structs exactly as C does; the C header is
  never included, so the Wlel declaration *is* the binding:

  ```wl
  struct Vector2 { x: f32, y: f32 }
  extern fn begin_drawing();
  extern fn draw_circle_v(center: Vector2, radius: f32, color: Color);
  ```

  ```bash
  wlel build examples/raylib.wl -o demo -l raylib -L raylib/lib
  ```

- **Linker flags**: `-l <lib>` and `-L <dir>` are repeatable on
  `wlel build`, `wlel run`, and `wlel test`, and pass straight through to
  cc (`-l raylib -L raylib/lib`).
- **Variadic C functions** (`printf`) cannot be declared — wrap them or
  use `std::format`.
- The generated prototype is non-`static`; a repeated *compatible*
  declaration is fine (the prelude already declares libc), but never
  redeclare a libc typedef — bind a name the headers do not define.

## Projects

A project is a directory with a `wlel.toml`. Scaffold one and run it:

```bash
$ wlel new myapp
created new project in ./myapp
next: cd myapp && wlel run
$ cd myapp && wlel run
hello, wlel!
```

Layout:

```text
myapp/
  wlel.toml      # manifest: name, version, deps
  src/main.wl    # binary entry (run/build)
  src/lib.wl     # library entry (optional; check/test work on library-only projects)
  .wlel/deps/    # git-dependency clones (generated — gitignore it)
  wlel.lock      # resolved git revisions (generated — commit it)
```

All verbs work in project mode when no `.wl` file is given — `wlel run/build/
check/test` look for `wlel.toml` in the current directory (or any parent).
`wlel build` writes a binary named after the package; `-o` overrides it.
Library-only projects (with `src/lib.wl` but no `src/main.wl`) can be checked
and tested but not built/run.

Dependencies are declared in `[deps]` — either a path to another wlel project
or a git URL (cloned automatically into `.wlel/deps/<name>`):

```toml
[package]
name = "myapp"
version = "0.1.0"

[deps]
pointlib = { path = "../pointlib" }
json     = { git = "https://github.com/user/wlel-json" }
```

Dependency functions, structs and tests merge into your program (like file
imports, diamond-deduped), nested dependencies resolve recursively, and two
dependencies sharing a name from different sources are rejected. Git
revisions are pinned in `wlel.lock` after the first build — commit it, and
every later build checks out exactly that revision, even on machines that
have never seen the repo. Delete the lock to move to the remote's HEAD.

## Formatting

`wlel fmt` re-prints source from the parse tree, so its output is canonical
and **idempotent** — formatting twice changes nothing:

```bash
$ wlel fmt src/main.wl        # format in place
formatted: src/main.wl
$ wlel fmt src/main.wl -check # CI mode: exit 1 if not canonical
all formatted
$ wlel fmt                    # project mode: every .wl under wlel.toml
```

The rules are deliberately few: 4-space indent, one statement per line,
exactly one blank line between top-level declarations (adjacent `use` lines
stay grouped; blank lines inside blocks collapse to one), `//` comments kept
(above the construct they precede, or trailing where they were), and
parenthesization rebuilt from precedence — `(a + b) * c` keeps its parens,
`a + (b * c)` loses them. Width-suffixed literals stay attached (`10u8`,
`2.5f32`), floats always print float-typed (`1e3` → `1000.0`), and radix
literals normalize to decimal (`0xFF` → `255`). A file that does not parse
is never touched.

## Warning system

`wlel check`, `wlel build`, and `wlel run` report warnings without failing the build:

```
$ wlel check main.wl
main.wl:7:5: warning: unused variable 'unused'
main.wl:1:1: warning: import 'geom.wl' is never used
ok: main.wl type-checks
```

- **Unused variable / parameter** — declared but never read (a plain write
  `x = 5;` does not count as a read).
- **Unused import** — `use std;` with no `std::` call, or `use "file.wl"`
  contributing no referenced function or struct.
- **Unreachable statement** — code after `return`/`break`/`continue` in the same block.
- **Opt-out** — prefix a name with `_` (`_x`, `_i`, `_cb`) to mark intentional
  non-use.

## Safe-debug vs release

Wlel compiles the same program two ways:

- **`wlel run` / `wlel test` (dev, safe)** — instrumented C. Every `[T; N]`
  array access is bounds-checked, every integer `/` and `%` is div-zero
  checked, and freed arena memory is poisoned with `0xDE` so use-after-free
  reads deterministic garbage. Failures abort with the exact `.wl` position:

  ```
  $ wlel run main.wl
  bounds check failed at main.wl:12: index 7, length 3
  $ wlel run div.wl
  division by zero at div.wl:4
  ```

- **`wlel build` (release)** — none of that exists in the generated C
  (verify with `--emit-c`: zero check helpers, zero poison). Same binary
  speed as before, same IEEE float semantics (`1.0/0.0` is `inf` in both
  modes — only integer division is guarded).

Notes:
- Pointers from `new(T, n)` have no tracked length, so they are not bounds
  checked — only static `[T; N]` arrays are.
- The checks are lazy (part of the access expression), so short-circuit
  semantics are preserved.
- When the optimizer can prove an index in range, even the dev build's
  check is folded away. On a check-heavy microbenchmark (data-dependent
  indices, divisions in the hot loop) dev costs ~30%; typical code is at
  or near parity (fib(35): release ≈ C).

## Arena stats

`arena_stats()` reports the innermost active arena (the root arena when no
block is open). `ArenaStats` is a built-in struct — the name is reserved:

```wl
arena(4096) {
    xs := new(int, 100);
    let s: ArenaStats = arena_stats();
    std::println_int(s.bytes);   // 800 — int is 8 bytes
    std::println_int(s.chunks);  // 1
    std::println_int(s.peak);    // 800 — high-water mark
}
```

For memory debugging beyond the built-in checks, `wlel build -sanitize`
produces an AddressSanitizer-instrumented binary (the same binary still
passes LeakSanitizer at exit thanks to the root arena):

```bash
wlel build app.wl -o app -sanitize && ./app
```

## Toolchain notes

- Generated C is meant to be read: `wlel build --emit-c` writes the C99 source
  next to your binary, with `#line` directives so compiler diagnostics point
  back into your `.wl` files.
- `wlel run` caches compiled binaries by content hash — rebuilds only happen
  when the source actually changes.
- The compiler never rejects a program on warnings, and one syntax error never
  hides the next: the parser recovers and reports everything it finds.

## Roadmap

Ordered by impact-per-effort. The next milestones:

1. **Stdlib & tooling** — generics via monomorphization, `Vec[T]`/`HashMap[K,V]`, string library, file I/O, `wlel fmt`, CI, FFI.
2. **Expressiveness** — tagged enums + exhaustive `match`, `Result[T,E]`, method sugar, LSP, concurrency.
3. **1.0** — three flagship apps (HTTP server, JSON parser, a small game), fuzz soak, full docs, cross-compile recipes.

Deliberately **not** on the roadmap: borrow checker, macros, GC, OOP class
hierarchies, exceptions. Simplicity is a feature, not a phase.
