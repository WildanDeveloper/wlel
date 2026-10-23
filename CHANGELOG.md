# Changelog

All notable changes to Wlel are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Versions before 1.0 shipped as a single evolving language; only the
milestones that matter to users are listed.

## [1.1.0] — 2026-10-23

The first ecosystem release: the project is now legally usable, the
dependency lifecycle is complete, and a new project ships with CI.

### Added

- **Apache-2.0 license** (`LICENSE`, `NOTICE`) plus the Cargo metadata a
  published package needs (`license`, `repository`, `homepage`, `keywords`,
  `categories`, `rust-version`). Generated C and compiled binaries remain
  yours; the license covers the compiler.
- **`wlel init [dir]`** — adopt an existing directory: writes `wlel.toml`,
  a README, `.gitignore` and a CI workflow, and only creates
  `src/main.wlel` when there is no entry point yet. Never overwrites
  existing files, and refuses a directory that already has a manifest.
- **`wlel remove <name>`** — drops the dependency line, its `wlel.lock` pin
  and its `.wlel/deps` clone in one step, leaving every other byte of the
  manifest untouched (the edited text is re-parsed before it is written).
- **`wlel update [name]`** — re-resolves git dependencies to the newest tag
  satisfying their requirement, fetches tags, keeps the requirement, and
  reports `old -> new` revisions. The previous lock is restored on failure.
- **`wlel tree`** — the resolved dependency graph with path/git sources,
  manifest versions and pinned revisions; unresolved clones and repeated
  nodes are labelled instead of failing the command.
- **`wlel clean [--cache]`** — removes the generated clone cache
  (`.wlel/`) while keeping `wlel.lock`; `--cache` also clears the
  compiler's content cache.
- **Scaffold**: `wlel new` / `wlel init` write a project README and a
  GitHub Actions workflow that installs the exact compiler release that
  generated the project, then runs `wlel check`, `wlel test` and
  `wlel fmt -check`.
- **Repository hygiene**: `CONTRIBUTING.md` (the change checklist and the
  commit-message convention), `SECURITY.md` (the compiler is treated as a
  parser of hostile input), `CHANGELOG.md`, bug/feature issue forms, a
  pull-request template, `.editorconfig`, `rust-toolchain.toml` and
  Dependabot for the fuzz crate and the pinned Actions.

### Changed

- Version 1.0.0 -> 1.1.0 (additive; no language or CLI behavior changed for
  existing projects).

## [1.0.0] — 2026-10-23

The 1.0 freeze: the language and its specification are frozen, and the
toolchain is distributed as prebuilt binaries.

### Language

- **Mixed float widths take the wider side.** `f32 + f64` is `f64`, `f32`
  widens into `f64` implicitly (assignment, arguments, returns), and an
  untyped float literal adapts to the other operand. Narrowing back to
  `f32` is an explicit cast. Previously the *left* operand decided the
  width, which truncated `f64` values and made `a + b` / `b + a` differ.
- **Explicit array decay:** `arr as *T` is legal anywhere, complementing the
  argument-only implicit decay. Element types must match exactly.
- Comparisons and `assert_eq` follow the same widening rules.
- Frozen decisions recorded in `docs/spec.md` §17: evaluation order stays
  C-inherited, lvalue checks are the compiler's, pointer arithmetic stays
  out, escape analysis is post-1.0.
- **Source extension is `.wlel`** (was `.wl`, which GitHub Linguist and most
  editors claim for Wolfram Language). The compiler rejects `.wl` with a
  rename hint.

### Compiler

- Defined arithmetic for the boundary cases C leaves undefined: `MIN / -1`
  wraps to `MIN` (`MIN % -1` is `0`) instead of trapping with `SIGFPE`, and
  shift counts are masked to the operand width (`x << 64` is `x`).
- The QBE backend agrees with the C backend on mixed float widths,
  `MIN / -1`, shifts, comparison widths and `print_float(f32)`.
- Compiler robustness: a self-growing generic instantiation now fails with a
  diagnostic instead of exhausting memory, and a long left-associative
  expression chain fails with "expression nested too deeply" instead of
  overflowing the stack.
- Infinite-size structs and enums (`struct N { next: N }`, `enum E { A(E) }`,
  and their generic forms) are rejected by the checker with the cycle named,
  instead of failing inside `cc` with a missing type.
- Reserved names: `__` in a declaration (the monomorphization separator),
  builtin type names, C keywords/runtime symbols and `main` as a type name.
- The build cache lives in the user's cache directory with `0700`
  permissions (it used to be a world-writable `/tmp/wlel_cache` whose file
  names are a public content hash), and git dependency URLs cannot inject
  git options.
- Manifest validation: package names cannot escape the project directory,
  versions must parse, and inline dependency tables reject duplicate keys
  and trailing text.

### Tooling

- `wlel doc` and LSP hover emit `wlel` code fences.
- `wlel fmt`, `wlel check`, `wlel test`, `wlel lsp`, `wlel doc` and project
  mode are unchanged in scope; diagnostics gained fixes for duplicated
  spans, radix-literal messages and `#line` escaping.
- Release pipeline: `scripts/release.sh` builds per-target archives with
  SHA-256 checksums; a tag-driven workflow builds Linux x86_64/aarch64,
  macOS Intel/Apple Silicon and Windows x86_64, gates on the test suite and
  refuses to publish when the tag disagrees with `Cargo.toml`.
- `docs/cross-compile.md` documents building the compiler for another
  platform and cross-compiling Wlel programs (`$CC`, `--emit-c`).

### Fixed

- `i64::MIN` division, out-of-range shifts, safe-mode width preservation,
  recursive generic/struct/enum cycles, name-mangling collisions, cache
  poisoning, git argument injection, manifest traversal, and eight
  diagnostic defects found by a full-source audit (see the commit history).

## [0.7.0] — 2026-10-23

- `wlel --version` / `-V` / `version`, stamped from `Cargo.toml`.
- Content-cached `wlel run` and `wlel build`.

## [0.3.0] — 2026-08-24

- Tagged enums + exhaustive `match`, `Result`/`Option`, the `?` operator.
- `impl` methods, concurrency, LSP, `wlel doc`, the git-tag package
  registry, the QBE backend, and the first specification draft.

## [0.1.0] — 2026-08-23

First tagged release: arenas as syntax, `defer`, structs, pointers, arrays,
modules, `use std`, and C99 code generation.
