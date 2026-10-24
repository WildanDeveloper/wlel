# Changelog

All notable changes to Wlel are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Versions before 1.0 shipped as a single evolving language; only the
milestones that matter to users are listed.

## [1.10.0] — 2026-10-24

The codegen stage joins the lexer, the parser and the checker: the compiler's
whole front end and its first back end are written in Wlel, and the C it
produces is proven to behave like the Rust compiler's output.

### Added

- **`selfhost/codegen.wlel`** — a Wlel → C generator over the checker's node
  arena: the runtime prelude, `c_type` built on interned type indices instead
  of re-parsing a spelling, `std__`/`geom__` mangling, forward-declared
  structs and tagged enums, `match` as a GCC statement-expression, range
  `for`, automatic `->` through a pointer, `new`/`sizeof`, compound
  assignment. The checker writes each node's type back into the arena, so
  codegen never re-derives one.
- **`tests/codegen_parity_tests.rs`** — the gate is behavioural: the generated
  C is compiled and run, and stdout + exit status must match the Rust
  compiler's build for nine cases (arithmetic, control flow, structs,
  strings/bools, exit status, narrow widths, enums + match, range for,
  pointers + `new`), plus a throughput guard. A mutation check (turning `+`
  into `-`) confirms the gate fails when it should.
- **Not yet covered** by the codegen: `defer`, `try` (`expr?`), array types
  `[T; N]`, `for-in`, `impl` methods, statement-form `match`, and the arena
  statement.

### Fixed

- **`impl` method docs stayed in the body.** The formatter flushed a pending
  `///` before a method only after the body's first statement, so
  `/// the payload` written above `fn unwrap` landed above its `return` and
  the doc generator no longer read it. `impl_def` now flushes before each
  method, and `lib/std.wlel` is restored to its pre-format revision and
  re-formatted with the fixed printer.

### Changed

- Version 1.9.1 -> 1.10.0 (additive; the Rust compiler is still the one that
  ships).

## [1.9.1] — 2026-10-24

Patch release: the checker parity gate failed on Windows.

### Fixed

- `relative()` stripped a `root()/` prefix that never matched on Windows,
  where the checker reports `D:\a\wlel\wlel\examples\...`, so all twenty
  known gaps were read as regressions. Both sides are slashed first now, and
  a unit test pins the Windows form, the Unix form and an out-of-tree path.
  The test also asserts every `KNOWN_GAPS` entry names a file that exists, so
  a typo cannot silently disable a gap.
- The `lib/std.wlel` exclusion in `sources()` compared an absolute path with a
  relative string and so never excluded anything.

## [1.9.0] — 2026-10-24

The checker joins the lexer and the parser: three of the compiler's four
stages are now written in Wlel and proven against the Rust ones.

### Added

- **`selfhost/checker.wlel`** — a type checker over the parser's node arena:
  scopes and shadowing, literal widths and suffixes, arithmetic and comparison
  with the mixed-width rules, bitwise and unary operators, casts, calls and
  arity, struct fields (including `p.x` on a `*S`), indexing (a string indexes
  to a byte), `let`/assignment/`return` compatibility, `if`/`while`
  conditions, function signatures, `match` arms with pattern binding, enum
  construction, generic signatures with `T` substitution, and the
  `std::`/`sys::` prelude. 24 MB/s over `flagship/json.wlel`.
- **`tests/checker_parity_tests.rs`** — asks both checkers about **every**
  `.wlel` file in the repository and compares their first diagnostic,
  `file:line:col: message`, exactly; twenty broken programs pin the
  diagnostics themselves; a `KNOWN_GAPS` ratchet names the constructs the
  stage does not cover yet and fails when one starts agreeing, so a fixed gap
  cannot be forgotten. Plus a throughput guard.
- **`wcheck <file> [-v|bench [n]]`** — the checker's CLI.
- **`lib/std.wlel`** — the standard library is now a real file, included with
  `include_str!`. The self-hosted stages need to read it, and the editor
  tooling can now see it; the compiled bytes are unchanged.

### Changed

- Version 1.8.1 -> 1.9.0 (additive; the Rust compiler is still the one that
  ships).

## [1.8.1] — 2026-10-24

Patch release: v1.8.0 was tagged before the parser performance fix, so its
binaries shipped the slower parser.

### Fixed

- The self-hosted parser no longer copies every name into a `String`: a node
  stores a source slice (`slice_start`/`slice_len`) and only materialises text
  when the dump needs it. That was 2 MB/s on Windows against 37 MB/s on Linux;
  it is now 53 MB/s on the same file. `wlel build` output is unchanged — the
  fix is inside `selfhost/`, which the compiler does not use at runtime — but
  the release archives now contain the version the repository documents.

## [1.8.0] — 2026-10-24

The parser joins the lexer: the front end of the compiler is now written in
Wlel and proven identical to the Rust one, file by file.

### Added

- **`selfhost/parser.wlel`** — a complete recursive-descent parser: items
  (`module`, `use` with `as` and import lists, `const`, `struct`, `enum`,
  `impl`, `extern`, `fn`, `test`), every statement form, all expression forms
  with the reference precedence table, `match` and patterns, struct literals,
  generic calls, `new`/`wlel_sizeof`/`wlel_arena`, function-pointer types, and
  the suffixed-literal rewrite (`10u8` parses as a cast, as the Rust AST
  records it). It reuses `selfhost/lexer.wlel` as a module, so the whole front
  end is Wlel.
- **Flat node arena** — the tree is one `Vec[Node]` with index children and
  `next` sibling links, and token text is a slice of the source. No per-node
  allocation; 79 300 nodes for `flagship/json.wlel` in one vector.
- **`tests/parser_parity_tests.rs`** — builds the Wlel parser, dumps the tree
  for **every** `.wlel` file in the repository (41 today) and compares it line
  for line against a Rust-side dumper of `wlel::ast`: item order, node shapes,
  operator codes, type spellings and literal spellings. Plus a throughput
  guard (`37 MB/s`, fails below 3).
- **`wparse <file> [dump|bench [n]]`** — node count, canonical AST dump, and an
  in-process benchmark.

### Fixed

- **Codegen mangled user strings.** The module-name mangling pass rewrote
  `"::"` to `"__"` inside *any* string argument of an `_wlel_*` helper, so
  `_wlel_strcat(a, "::")` silently produced `"__"` in the user's data. It now
  applies only to the helpers that really carry symbol names (`_wlel_sort`,
  `_wlel_bsearch`, `_wlel_thread_spawn`, `_wlel_chan_new`). Found by the parser
  parity dump.
- **Duplicate `const` names were accepted.** Two `const X` declarations in one
  module silently kept the last value; the checker now reports
  `duplicate const 'X'`. Found while the self-hosted parser had two constants
  with the same name.

### Changed

- Version 1.7.0 -> 1.8.0 (additive; the Rust compiler is still the one that
  ships).

## [1.7.0] — 2026-10-24

Self-hosting begins: the lexer is now written in Wlel, and it is proven
identical to the Rust one before any other stage is started.

### Added

- **`selfhost/lexer.wlel`** — a complete Wlel lexer: trivia and `//` comments,
  every literal form (decimal, hex, binary, octal, `_` separators, fractions,
  exponents, width suffixes), string escapes, every operator including the
  multi-character ones, all keywords including `module` and `const`, and error
  positions for invalid tokens, bad radix digits, unterminated strings and
  unknown suffixes. Two design choices make it fast: tokens are **zero-copy**
  (`{ kind, start, end, line, col }` — byte offsets, no per-token `String`),
  and token kinds are `const` integers rather than an enum with payloads.
- **`tests/selfhost_tests.rs`** — the parity gate, 6 tests: the Wlel lexer is
  built and run over **every** `.wlel` file in the repository (40 today) and
  compared token-for-token against `wlel::lexer` (boundaries, line/column,
  class and text); a token-count check; a set of tricky cases (radix and
  suffixed literals, escapes, `a::b::c`, `x..y`, `<<=`, comments, `match`
  arms, `const`/`module`, every operator); rejection of an invalid token; and
  a throughput guard.
- **`wlex <file> [dump|bench [n]]`** — the lexer's CLI: token count, the
  `line|col|start|end|class|text` dump the parity test consumes, and an
  in-process benchmark.
- **`selfhost/README.md`** — the staging plan (lexer → parser → checker →
  codegen, each gated by parity) and the measurements.

### Performance

100 passes over `flagship/json.wlel` (7 257 tokens each) in 32 ms: **116 MB/s**
with one allocation per pass, because nothing is copied per token. A guard
test fails if throughput drops below 10 MB/s, so a regression to per-token
allocation or quadratic trivia scanning is caught.

### Changed

- Version 1.6.0 -> 1.7.0 (additive; the Rust compiler is unchanged and still
  the one that ships).

## [1.6.0] — 2026-10-24

The last three language gaps on the self-hosting checklist: compile-time
constants, function pointers, and a string builder that is not quadratic.

### Added

- **`const NAME: type = expr;`** — a compile-time constant. The value must be
  a literal, another constant, a cast to a builtin type, or arithmetic on
  those; it is folded and substituted at every use, so a lookup table costs
  nothing at runtime. Constants may be defined in terms of earlier ones
  (`const DOUBLE: int = LIMIT * 2;`), cross module boundaries
  (`limits::MAX`, or the bare name when imported), and are printed by
  `wlel fmt`. A cycle, a type mismatch, or a non-foldable value (a call, an
  index) is a clear error rather than a confusing runtime failure.
- **Function pointers** — `fn(int, string) -> float` as a type. A top-level
  function name is a value of that type (`f := inc;`, `apply(inc, 10)`), and
  `f(x)` is an indirect call. They work as parameters, locals, struct fields
  and return types, a function-pointer field is callable with dot syntax
  (`obj.cb(x)`), and the emitted C carries the declarator correctly
  (`long long (*pick(long long))(long long)`). Calling a non-function value
  and a wrong arity through a pointer are compile errors. A function pointer
  cannot be a generic argument (`Vec[fn(int) -> int]`) — the message explains
  the wrapper-struct form and why.
- **`std::str_buf_*`** — an amortized string builder (`str_buf_new`,
  `str_buf_push`, `str_buf_push_int`, `str_buf_push_float`,
  `str_buf_push_char`, `str_buf_len`, `str_buf_finish`). Building a string by
  repeated `s = s + piece` copies the whole buffer each time; 20 000 appends
  through the builder take milliseconds where concatenation took seconds.
  The builder is freed by `str_buf_finish`.
- **`examples/constants.wlel`** and **`examples/function_pointers.wlel`**,
  plus `tests/lang_ext_tests.rs` — 15 tests covering const folding, chains,
  modules, cycles, type mismatch and non-foldable values; function pointers
  as values, parameters, return types, struct/vec fields, dot-syntax calls,
  plus the two error cases; and the builder's correctness, length and
  linearity.

### Changed

- The tree-sitter grammar gained `const_declaration` and `function_type`;
  every `.wlel` file in the repository still parses with zero errors.
- Spec §6.11 documents constants, §16's type grammar gained the function
  type with its restrictions, and README has a
  "Constants and function pointers" section.
- Version 1.5.1 -> 1.6.0 (additive; no existing program changes meaning).

## [1.5.1] — 2026-10-24

Patch release for the host runtime, all three found by the Windows runner:

### Fixed

- `sys::mkdir` did not compile on Windows/MinGW — `_mkdir` lives in
  `<direct.h>` (and `_access` in `<io.h>`), which the spliced runtime did not
  include, so any program touching it failed at cc time.
- `sys::remove` deleted files but not directories on Windows (`remove()` is
  the wrong CRT call there; `_rmdir` is used instead).
- A local named `stdin`, `stdout` or `stderr` shadowed MinGW's stdio macro
  and made the generated C unparseable. Those names (plus the libc functions
  the runtime itself calls) are now rejected by the checker with a normal
  diagnostic instead of a cc error pointing into generated C.

## [1.5.0] — 2026-10-24

Two gaps that blocked self-hosting: enum variants were one flat global
namespace, and a Wlel program could not talk to the host beyond files.

### Added

- **Host integration (`sys::`)** — a program can now drive a toolchain:
  - `sys::spawn(cmd, &out) -> int` runs a shell command line, captures stdout
    into `out`, and returns the exit status (`-1` when it cannot start);
    `sys::run(cmd)` is the no-capture form. This is the capability `wlel
    build` itself needs to invoke `cc`.
  - `sys::getenv(name) -> string`, `sys::read_dir(path) -> string`
    (newline-separated entries), `sys::read_line()` / `sys::read_stdin()` for
    stdin, and `sys::eprint/eprintln` for stderr — diagnostics no longer have
    to go to stdout.
  - `sys::mkdir/remove/rename/exists` for filesystem changes.
  - The runtime is spliced into the generated C only when a program calls one
    of them (a test asserts the unused case stays clean), and the API is
    total: failures return `false`/`""`/`-1` rather than trapping. Verified
    end to end by a Wlel program that writes C, invokes `cc`, runs the binary
    and cleans up.
- **Enum variants are namespaced by their enum** — `Tok::Ident(x)` and
  `Expr::Ident(x)` are different constructors, in patterns and in expressions,
  and two enums in one file may now share a variant name. A bare `Ident` still
  works while exactly one enum claims it; once two do, the error names both
  and shows the qualified form. `module::Variant` also resolves (the enum is
  looked up through the module's exports).
- **`std::` reaches the whole library** — `std::str_trim`, `std::vec_new`,
  `std::int_to_str`, `std::str_split`, ... now work, not just the handful of
  runtime builtins. `std::name` and the plain `name` are the same function, so
  both spellings are valid after `use std;`.
- **`tests/host_tests.rs`** — 9 end-to-end tests: spawn capture + status, run
  status, driving `cc` from Wlel, `getenv`, mkdir/exists/rename/remove,
  `read_dir`, stdin line/whole, stderr routing, and "the runtime is only
  spliced when used".

### Changed

- The spec gained §9.5 (host integration), the variant namespace rules in
  §8.2, and the grammar note; the sys table in README documents the new
  functions.
- Version 1.4.0 -> 1.5.0 (additive; no existing program changes meaning).

## [1.4.0] — 2026-10-24

Module namespaces: a program is no longer one flat scope. Two files may define
the same name, and every file's items are reachable through its module. This
was the last blocker for self-hosting.

### Added

- **`module name;`** — every file belongs to a module. The declaration names
  it; without one the file stem is used (`lib/geom.wlel` -> `geom`). The file
  you run/check/build is the *root module*, whose items stay unqualified, so
  `fn main` is still the C entry point and single-file programs are unchanged.
- **Qualified access** — `geom::add(...)`, `geom::Vec2 { ... }`,
  `geom::Vec2` as a type annotation, `impl geom::Vec2`, and
  `geom::Circle(1.0)` / `geom::Circle(r) => ...` for enum variants.
  Declarations are mangled to `geom__add` / `geom__Vec2` in the generated C,
  so two modules with the same name never collide at link time. The QBE
  backend and the LSP see the same resolved program.
- **Import forms** — `use "lib/geom.wlel";` binds the module;
  `as g` renames it; `{ add, Vec2 }` also brings those names into scope;
  `{ add as plus }` renames an import; `{ * }` imports every export;
  `use geom;` names a module or a package dependency. File imports stay
  recursive and diamond-deduplicated (one module merged once).
- **Deliberate collisions** — a bare name resolves to the current module's
  items first, then to imports. Two imported modules providing the same name
  is an error that names both candidates and shows the fix; the qualified
  form always resolves. A name from a module that was never imported is an
  error that names the module (`'hi' is in module 'greetx'; write greetx::hi
  or add `use greetx;``), and a module referencing another module's item
  without importing it is rejected.
- **Module-aware diagnostics** — duplicate `module` declarations across files,
  reserved names (`std`, `sys`), `module` not first, a second `module` line,
  and importing an item a module does not export all have dedicated messages.
- **`examples/modules.wlel`** plus `examples/lib/shapes.wlel` and
  `examples/lib/units.wlel`: two modules defining the same `helper`, reached
  qualified, through an alias, and through an import list.
- **tree-sitter**: `module_declaration`, named/qualified imports, qualified
  types and qualified struct-literal names, plus a corpus case and
  highlight queries for modules and imported names.
- **`tests/module_tests.rs`** — 29 tests covering the same name in two
  modules, ambiguity, qualified access, import lists/aliases/globs, root
  scope, module-local visibility, nested and diamond imports, enums, methods,
  generics, tests in imported modules, project dependencies, reserved and
  duplicate module names, formatter round-trips, C mangling, and the
  pre-existing examples.

### Changed

- **Project dependencies are modules.** `use geom;` then `geom::mk(...)`;
  the package name is the module name. A bare `mk(...)` is now an error that
  says which module has it (it used to work because dependencies were
  flattened into the program).
- `wlel fmt` prints the `module` line and every import form verbatim;
  `wlel doc` keeps one page per file; the language server resolves the same
  module names as the CLI.
- The spec's §9 is rewritten (files and modules, resolution order, imports)
  and §16's grammar gains `module_decl` and the import forms.
- Version 1.3.0 -> 1.4.0.

## [1.3.0] — 2026-10-23

Distribution: Wlel is now one command away on every platform, and the front-end
runs in a browser tab.

### Added

- **`install.sh`** — one-line install for Linux and macOS. It resolves the
  latest release through the `/releases/latest` redirect (no API rate limit),
  picks the archive for the host, verifies it against the release's
  `SHA256SUMS` with `sha256sum` or `shasum`, and installs to `/usr/local/bin`
  when writable and `~/.local/bin` otherwise. Never runs `sudo`; `WLEL_VERSION`
  pins a release and `WLEL_INSTALL_DIR` picks the directory.
- **Homebrew and Scoop manifests** — `scripts/packaging.sh` renders
  `packaging/homebrew/wlel.rb` and `packaging/scoop/wlel.json` from a release's
  `SHA256SUMS`, so the URLs and per-architecture hashes cannot drift from the
  archives. `packaging/README.md` documents publishing the tap and bucket.
- **Documentation site** — `scripts/docsite.py` renders the repository's
  markdown (README, spec, changelog, contributing, security, cross-compile and
  the three editor READMEs) into a static site with a shared header, per-page
  table of contents and syntax-tagged code blocks, using only the Python
  standard library. `scripts/site.sh` builds it plus the playground, and the
  new `docs` workflow publishes to GitHub Pages with `keep_files` so the
  benchmark trend that already lives on `gh-pages` survives.
- **WebAssembly playground** (`playground/`) — the real front-end (lexer,
  error-recovering parser, `use std` splice, type checker) compiled to
  `wasm32-unknown-unknown` behind a five-function C ABI (`wlel_alloc`,
  `wlel_analyze`, `wlel_result_ptr`, `wlel_dealloc`, `wlel_version`), with no
  wasm-bindgen and no JS build step. The page shows diagnostics with
  click-to-jump positions, offers `wlel fmt` output, and runs four examples;
  the whole module is ~470 KiB.
- **`src/playground.rs`** — the analysis pipeline the playground exposes, as
  JSON: `{"ok":…,"diagnostics":[…],"formatted":…}`. A file import is a clean
  diagnostic rather than a filesystem read, and a panic inside the pipeline is
  caught and reported instead of trapping the module.
- **Tests** — `tests/playground_tests.rs` (8 tests: ok/warn/error/lex/syntax,
  import rejection, JSON escaping) and `tests/distribution_tests.rs` (8 tests:
  POSIX syntax of both scripts, archive layout agreement with `release.sh`,
  manifest rendering with a synthetic `SHA256SUMS`, doc-site rendering and
  internal links, playground ABI agreement between Rust and JS, and a real
  wasm build). A new `distribution` CI job runs both.

### Changed

- `scripts/packaging.sh` writes `packaging/` instead of `dist/packaging/`
  (`dist/` is build output and git-ignored); override with
  `WLEL_PACKAGING_OUT`.
- `docs.yml` and `bench.yml` share a `gh-pages` concurrency group so the two
  publishers cannot race each other.
- Version 1.2.0 -> 1.3.0 (additive; no language or CLI behavior changed).

## [1.2.0] — 2026-10-23

Editor support: Wlel now highlights, navigates and completes in the editors
people already use, and the grammar is registered everywhere it needs to be.

### Added

- **Editor support** (`editors/`), all three under Apache-2.0:
  - **`editors/vscode-wlel`** — a VS Code extension with a TextMate grammar
    (`source.wlel`) and a zero-dependency language client over `wlel lsp`:
    diagnostics, hover, go-to-definition and completion, with
    `wlel.serverPath` / `wlel.serverArgs` / `wlel.trace.server` settings and
    a restart-on-config-change path. No `tsc`, no `node_modules`.
  - **`editors/tree-sitter-wlel`** — a tree-sitter grammar (`source.wlel`)
    that parses every file in `examples/` and `flagship/` with zero `ERROR`
    or `MISSING` nodes, with `queries/highlights.scm`, `queries/locals.scm`,
    corpus tests and committed generated `src/`. Works from Neovim, Helix,
    Emacs and Node.
  - **`editors/linguist`** — the GitHub Linguist registration kit
    (`languages.yml`, `grammars.yml`, sample staging script, PR checklist).
    `.gitattributes` already marks `*.wlel` as Wlel in this repository.
- **Editor tests** (`tests/editor_tests.rs`) — 12 tests that validate the
  manifest, TextMate grammar, language configuration and JavaScript, load the
  extension against a stub `vscode` module and drive it against the real
  `wlel lsp` (hover, definition, completion, diagnostics), compile the
  generated tree-sitter parser as C, regenerate the grammar to catch a stale
  `src/`, parse every `.wlel` file in the repository, and compile both query
  files. Tests needing `node`, `cc` or `tree-sitter` skip cleanly when the
  tool is absent, and a new `editors` CI job installs them and runs the
  suite for real.

### Changed

- Version 1.1.0 -> 1.2.0 (additive; no language or CLI behavior changed).

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
