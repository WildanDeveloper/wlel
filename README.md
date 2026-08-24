# Wlel

A fast, simple systems programming language. Compiles to native binaries via a
C backend — performance is indistinguishable from hand-written C.

```wl
fn fib(n: int) -> int {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> int {
    wlel_print_int(fib(28));
    return 0;
}
```

## Status (v0.3)

- [x] Lexer (ints, floats, strings w/ escapes + UTF-8, comments)
- [x] Recursive-descent + precedence-climbing parser → AST
- [x] Type checker: inference (`x := e`), annotations, scoped locals, return-path analysis
- [x] Codegen: transpile to C99 (`cc/gcc/clang -O2`) with exact types
- [x] Structs (nominal, C layout), struct literals, field access with auto-deref
- [x] Pointers: `&`, `*`, `*T` types, auto-deref fields
- [x] Arrays `[T; N]`: literals, indexing, `std::len`, C-style decay to pointer args
- [x] Casts: `expr as T` (numerics, pointers, int↔ptr)
- [x] Heap & arena: `wlel_alloc/wlel_free`, `wlel_arena_new/alloc/free` (bump arena w/ overflow chunks)
- [x] `wlel_sizeof(T)`
- [x] `defer` (LIFO, scope/loop/early-return aware), bare blocks for scoping
- [x] Modules: `use "path.wl";` merging, `use std;` stdlib (`std::print*/println*`, `streq`, `abs`, `min`, `max`, `len`)
- [x] CLI: `wlel run file.wl`, `wlel build file.wl -o out`
- [ ] QBE/LLVM backend option
- [ ] Generics, closures, string ops (streq-based ==)

## Try it

```bash
cargo run --release -- run examples/fib.wl   # -> 317811
```

## Language sketch

- declarations: `x := expr;` or `let x: T = expr;`
- control flow: `if / else if / else`, `while`
- functions: `fn name(a: int, b: int) -> int { ... }`
- builtins: `wlel_print_int(v)`
