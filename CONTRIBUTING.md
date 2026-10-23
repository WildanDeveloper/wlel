# Contributing to Wlel

Thanks for taking a look. Wlel is a small language with a deliberately
small compiler, and the bar for a change is: **it must keep the whole test
suite green and make the language simpler or more predictable, not just
bigger.**

## Getting set up

```bash
git clone https://github.com/WildanDeveloper/wlel
cd wlel
cargo build --release          # builds target/release/wlel
./target/release/wlel --version

cargo test --release --locked  # the whole suite (needs cc and git on PATH)
cargo clippy --all-targets -- -D warnings
find examples flagship -name '*.wlel' -print0 | xargs -0 ./target/release/wlel fmt -check
```

A C compiler (`cc`/`gcc`/`clang`) is required because Wlel transpiles to
C99; `git` is required for the dependency tests. Everything else is
dependency-free — the compiler, formatter, language server and doc
generator are plain Rust with no third-party crates.

## What CI enforces

Every pull request must pass:

- `cargo test --release --locked` on Linux, macOS and Windows.
- `cargo clippy --all-targets -- -D warnings` — warnings are errors.
- `wlel fmt -check` over `examples/` and `flagship/` — the formatter is
  canonical, so committed `.wlel` files must already be formatted.
- The flagship job: type-check, run every `test` block and build
  `flagship/*.wlel`, plus a JSON throughput smoke test.
- `scripts/release.sh` (the packaging path) in the lint job.

## Changing the language

The specification is normative: `docs/spec.md`. If a change affects
observable behavior, update the spec in the same pull request, and add a
section to `CHANGELOG.md`. Anything that changes syntax or semantics after
1.0 needs a strong reason — the syntax is frozen.

A language change usually touches several layers, and all of them must
agree:

1. `src/lexer.rs` / `src/token.rs` — if it needs new tokens.
2. `src/parser.rs` — the grammar (with error recovery and the nesting guard).
3. `src/ast.rs` — the node, if it is new syntax.
4. `src/checker.rs` — typing, plus the monomorphization and cycle rules.
5. `src/codegen.rs` — the C backend is normative; **and** `src/qbe.rs` if
   the construct belongs to the QBE subset.
6. `src/fmt.rs`, `src/lsp.rs`, `src/doc.rs` — the tooling must not lose it.
7. `docs/spec.md` — the normative description.

## Tests

- Put unit tests next to the layer they cover (`tests/lang_tests.rs`,
  `tests/checker_tests.rs`, `tests/qbe_tests.rs`, …).
- End-to-end behavior goes in `tests/cli_tests.rs` or a focused file.
- A bug fix needs a test that **fails before** the fix. Name it after the
  bug, not after the function (`min_div_minus_one_wraps_instead_of_trapping`).
- Anything touching generated C should be checked under the sanitizers:

  ```bash
  ./target/release/wlel build app.wlel -O1 --emit-c -o app.c
  gcc -O1 -std=gnu99 -fwrapv -fsanitize=address,undefined app.c -o app -lm -lpthread
  ```

- `wlel run` and `wlel test` compile in safe mode (bounds and division
  checks); `wlel build` is release mode. Test both when the behavior differs.

## Style

- Rust: `cargo fmt` plus the clippy gate. Comments explain *why* (especially
  where a C or LLVM behavior is surprising), not *what*.
- Generated C must stay readable — it is part of the product. `#line`
  directives must point back into the `.wlel` file.
- Error messages: lowercase, name the file and position, and say what to do
  ("store a pointer (`*N`) instead", "rename it: `mv a.wl a.wlel`").

## Commits and pull requests

- One logical change per commit. The subject line is the changelog entry:
  it states what changed and why, in the present tense, and ends with the
  verification (`548 cargo tests green, clippy -D warnings clean,
  wlel fmt canonical`).
- Reference the issue a pull request closes.
- Keep the diff reviewable; split mechanical renames from behavior changes.

## Reporting bugs

Include:

- the Wlel source that reproduces it (minimal),
- `wlel --version`, your OS, and the C compiler (`cc --version`),
- the exact command and the full output,
- if it is a miscompile: `wlel build --emit-c -o out.c` and the relevant
  part of the generated C.

Crashes of the compiler itself (a panic or an abort, rather than a
diagnostic) are always bugs — the compiler must report errors, never die.

## License

By contributing you agree that your contribution is licensed under the
Apache License 2.0 (see `LICENSE`), without additional terms.
