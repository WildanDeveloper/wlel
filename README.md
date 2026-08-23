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

## Status (v0)

- [x] Lexer (ints, floats, strings w/ escapes + UTF-8, comments)
- [x] Recursive-descent + precedence-climbing parser → AST
- [x] Codegen: transpile to C99 (`cc/gcc/clang -O2`)
- [x] CLI: `wlel run file.wl`, `wlel build file.wl -o out`
- [x] `defer` (LIFO, scope/loop/early-return aware), bare blocks for scoping
- [ ] Type checker (currently: untyped AST, everything `long long`)
- [ ] Pointers, structs, arrays
- [ ] Arena allocator (`defer`-based), modules/imports
- [ ] QBE/LLVM backend option

## Try it

```bash
cargo run --release -- run examples/fib.wl   # -> 317811
```

## Language sketch

- declarations: `x := expr;` or `let x: T = expr;`
- control flow: `if / else if / else`, `while`
- functions: `fn name(a: int, b: int) -> int { ... }`
- builtins: `wlel_print_int(v)`
