# Wlel

**The Arena-First Systems Programming Language.**

Wlel compiles to native binaries via a C99 backend — performance is indistinguishable
from hand-written C, but memory safety and performance are built around a unique core idea:
**first-class scoped arenas**.

```wl
use std;

struct Node { val: int, next: *Node }

fn main() -> int {
    // 1 MiB bump arena: all allocations inside are O(1) pointer bumps
    arena(1 << 20) {
        let head: *Node = 0 as *Node;
        i := 0;
        while i < 100_000 {
            n := new(Node); // typed, arena-aware!
            n.val = i;
            n.next = head;
            head = n;
            i += 1;
        }
        std::println_int(head.val);
    } // <-- ALL 100k nodes freed together in O(1)! Zero leak, zero GC pause.

    return 0;
}
```

### Mengapa ini unik?
- **C/C++**: butuh library terpisah, disiplin manual tinggi, rawan bocor.
- **Rust/Zig**: arena adalah library pattern (`ArenaAllocator`), bukan first-class syntax bahasa.
- **Go/Java**: bayar biaya Garbage Collector (GC latency, memory overhead 2x-3x).
- **Wlel**: arena adalah **syntax bahasa**: `arena(bytes) { ... }` + `new(T)`. Otomatis aman terhadap `return` dini, `break`, dan `defer` (LIFO free, zero leak terverifikasi via AddressSanitizer).

## Status (v0.6)

- [x] **First-class arenas**: `arena(size) { ... }`, `arena { ... }` (default 4 KiB), `wlel_arena()`
- [x] **Typed allocator**: `new(T)`, `new(T, count)` — sadar arena, fallback ke heap di luar arena
- [x] **Root arena**: string temporer & alokasi global hidup di root arena yang dibebaskan otomatis saat program selesai (`atexit`) — zero leak end-to-end
- [x] **Safe unwind**: return dini / `break` / `defer` di dalam arena mengevaluasi nilai balik SEBELUM arena di-free (zero use-after-free)
- [x] **for-in range loops**: `for i in 0..n { ... }` (bounds dievaluasi sekali, loop var ber-tipe int, aman dengan `break`/`continue`/`defer`)
- [x] **String ops**: `+` concat (arena-aware), `==` / `!=` (strcmp), `std::strlen` — ditutup dari TODO v0.3
- [x] **sys module**: `sys::argc()`, `sys::arg(i)`, `sys::exit(code)` — program CLI nyata
- [x] **Reserved identifiers**: prefix `_wlel_` dilindungi compiler
- [x] Forward struct declarations (rekursif `struct Node { next: *Node }` bekerja alami)
- [x] Lexer (ints, floats, strings w/ escapes + UTF-8, comments, hex/bin/oct literals, scientific, separator `_`)
- [x] Recursive-descent + precedence-climbing parser → AST
- [x] Type checker: inference, annotations, scoped locals, return-path analysis, loop-depth & arena-depth tracking
- [x] Codegen: transpile to C99 (`cc/gcc/clang -O2`) with exact types
- [x] Control flow: `if / else if / else`, `while`, `for i in a..b`, `break`, `continue`
- [x] Compound assignments: `+=`, `-=`, `*=`, `/=`, `%=`
- [x] Bitwise operators: `&`, `|`, `^`, `~`, `<<`, `>>`
- [x] Structs (nominal, C layout), struct literals, field access with auto-deref
- [x] Pointers: `&`, `*`, `*T` types, auto-deref fields
- [x] Arrays `[T; N]`: literals, indexing, `std::len`, C-style decay to pointer args
- [x] Casts: `expr as T` (numerics, pointers, int↔ptr)
- [x] Manual heap & arena: `wlel_alloc/wlel_free`, `wlel_arena_new/alloc/free`
- [x] `wlel_sizeof(T)`
- [x] `defer` (LIFO, scope/loop/early-return aware), bare blocks for scoping
- [x] Modules: recursive `use "path.wl";` merging with diamond-import deduplication, `use std;`
- [x] CLI: `wlel run` (cached binary), `wlel build file.wl -o out [-O0..-O3] [--emit-c]`, `wlel check file.wl`
- [x] Source spans & diagnostics: error `file.wl:12:5` di parser/checker + `#line` mapping agar error cc menunjuk balik ke `.wl`
- [x] Width types: `u8/u16/u32/u64, i8/i16/i32/i64, usize, f32/f64` (`int`=i64, `float`=f64, `byte`/`char`=u8); suffix `10u8`; literal adapt + range-check; pengecilan wajib cast
- [x] Overflow semantics: unsigned wrap mod 2^N, signed wrap via `-fwrapv`, `std::checked_add/sub/mul` untuk deteksi
- [x] Warning system: unused variable/parameter/import, unreachable code — non-fatal, `_`-prefix untuk opt-out
- [x] Parser error recovery: satu compile melaporkan semua error sintaks (bukan hanya yang pertama)
- [x] `defer` blok: `defer { ... }` multi-statement — LIFO lintas bentuk, blok punya scope defer sendiri
- [ ] Direct QBE/LLVM backend (eliminates C-compiler dependency)
- [ ] Generics, native growable string type `str` (mutable, slicing)

## Try it

```bash
cargo run --release -- run examples/arena.wl            # scoped arena demo
cargo run --release -- run examples/loops.wl            # for-in range demo
cargo run --release -- run examples/strings.wl          # string concat/eq demo
cargo run --release -- run examples/args.wl -- greet you # CLI args demo
cargo run --release -- run examples/fib.wl              # fibonacci benchmark
cargo run --release -- check examples/kitchen.wl        # type-check only
cargo run --release -- build examples/arena.wl -o arena -O3 --emit-c
```

## Language sketch

- declarations: `x := expr;` or `let x: T = expr;`
- control flow: `if / else if / else`, `while`, `break`, `continue`
- compound assignment: `x += 1;`, `f *= 2.0;`
- operators: `+ - * / %` (numeric), `& | ^ ~ << >>` (bitwise), `== != < > <= >=`, `&& || !`
- functions: `fn name(a: int, b: int) -> int { ... }`
- builtins: `wlel_print_int(v)`

## Semantik overflow

Aritmetika integer Wlel **tidak pernah panik dan tidak pernah UB** — hasilnya
terdefinisi penuh di dev maupun release:

| Kasus | Perilaku |
|-------|----------|
| Unsigned (`u8..u64, usize`) | wrap modulo 2^N (standar C) |
| Signed (`i8..i64`) | two's-complement wrap — backend dikompilasi dengan `-fwrapv` |
| Width < 32 bit | compiler menyisipkan cast wrap eksplisit, jadi wrap terjadi di width asli, bukan di `int` C |

Deteksi overflow tersedia via stdlib (tanpa exception, tanpa panic):

```wl
use std;

fn main() -> int {
    r := 0;
    if std::checked_add(9_000_000_000, 9_000_000_000, &r) {
        std::println_int(r);          // aman: r valid
    } else {
        std::println_str("overflow"); // r tidak berubah
    }
    return 0;
}
```

`std::checked_add/sub/mul(a: int, b: int, out: *int) -> bool` — false saat
overflow; hasil hanya dianggap valid saat return `true`.

## Warning system

`wlel check`, `wlel build`, dan `wlel run` melaporkan warning tanpa
menggagalkan build:

```
$ wlel check main.wl
main.wl:7:5: warning: unused variable 'unused'
main.wl:1:1: warning: import 'geom.wl' is never used
ok: main.wl type-checks
```

- **Unused variable / parameter** — dideklarasikan tapi tidak pernah dibaca
  (penugasan murni `x = 5` tidak dihitung bacaan).
- **Unused import** — `use std;` tanpa panggilan `std::`, atau `use "file.wl"`
  yang tidak menyumbang fungsi/struct yang dirujuk.
- **Unreachable statement** — statement setelah `return`/`break`/`continue`
  dalam blok yang sama.
- **Opt-out**: awali nama dengan `_` (`_x`, `_i`, `_cb`) untuk menandai
  ketidakterpakainya disengaja.

## Roadmap

Ordered by impact-per-effort, as decided in the v0.4 takeover review:

1. **Source spans & diagnostics** — thread `line:col` from lexer through AST nodes
   so parse/type errors point at real code, and emit `#line` directives in the
   generated C so cc errors map back to `.wl` sources. (Currently the AST has no
   positions; errors are position-free strings.)
2. **Native strings** — growable `str` with `len`, `+` concat, indexing to `u8`
   (needs a `byte`/`u8` type), replacing `const char*` string literals.
3. **`for` loops & ranges** — `for i in 0..n { ... }` desugared to while.
4. **Int width types** — `u8/u32/i8/i32` etc. (currently every `int` is `long long`);
   unlocks bit-exact data formats and pointer-width casts.
5. **Generics via monomorphization** — `fn id[T](x: T) -> T`; C backend makes
   this straightforward (recompile per instantiation).
6. **`sys::args`, `sys::exit`** — pass-through of process arguments (CLI already
   forwards `-- args` to the binary).
7. **Module namespaces** — qualified names (`math::sin`) instead of one merged
   namespace; needs parser symbol tables.
8. **Direct QBE backend** — drop the C toolchain dependency for `wlel run`.
9. **Self-hosting** — rewrite the compiler in Wlel (std + file I/O required first).
