# Self-hosting

The Wlel compiler is written in Rust. This directory is the Wlel rewrite, one
stage at a time, each stage gated by a parity test against the Rust
implementation before the next one starts.

| Stage | Source | Gate |
|---|---|---|
| lexer | `lexer.wlel` | `tests/selfhost_tests.rs` — token-for-token parity on every `.wlel` file in the repository |
| parser | `parser.wlel` | `tests/parser_parity_tests.rs` — AST dump parity on every `.wlel` file |
| checker | `checker.wlel` | `tests/checker_parity_tests.rs` — agreement on every `.wlel` file, plus an exact-diagnostic corpus |
| codegen | `codegen.wlel` | `tests/codegen_parity_tests.rs` — behavioural parity (stdout + exit code) over the supported core |

## The lexer

`lexer.wlel` is a complete lexer: trivia, all literal forms with width
suffixes, escapes, every operator, keywords, and error positions. It is also
the design testbed for the rest of the self-hosted compiler, so it takes two
deliberate shortcuts:

1. **Zero-copy tokens.** A token is
   `{ kind, start, end, line, col }` — byte offsets into the source. Nothing
   is allocated per token; the text is a slice when it is needed. The Rust
   lexer owns a `String` per identifier/string/number token. Lexing a file
   allocates exactly one `Vec[Tok]`.
2. **Kinds are integers.** `const T_FN: int = 10; …` gives the scanner a
   flat integer comparison instead of an enum with a payload, and makes the
   dispatch table a plain `if` chain the optimiser can fold.

Both are possible because Wlel now has `const` and a `Vec[T]` that is a plain
buffer — the features added for self-hosting are what make this shape
worthwhile.

### Performance

```
$ wlel build selfhost/lexer.wlel -O2 -o wlex
$ ./wlex flagship/json.wlel bench 100
100 passes, 725700 tokens, 32 ms, 116 MB/s
```

That is the whole lexer — process start excluded, symbol table included — at
116 MB/s on the largest file in the repository, with one allocation per pass.
The guard test fails if throughput drops below 10 MB/s, so a regression to
per-token allocation or O(n²) trivia scanning is caught.

### Usage

```sh
wlex <file>              # token count (or the lex error, with line:col)
wlex <file> dump         # line|col|start|end|class|text, one token per line
wlex <file> bench [n]    # n passes in-process, prints MB/s
```

`dump` is the format the parity test compares: `class` is one of `K`
(keyword), `I` (identifier), `N` (integer), `F` (float), `S` (string), `P`
(punctuation), `E` (eof). The test builds the Wlel lexer, runs it over every
source in the repository, and compares boundaries, line/column, class and
text against `wlel::lexer` — 40 files today.

## The parser

`parser.wlel` is a complete recursive-descent parser for the language: items
(`module`, `use` with aliases and import lists, `const`, `struct`, `enum`,
`impl`, `extern`, `fn`, `test`), statements, all expression forms with the
reference precedence table, `match`, patterns, struct literals, generic
calls, `new`/`wlel_sizeof`/`wlel_arena`, function-pointer types, and the
suffixed-literal rewrite (`10u8` becomes a cast, exactly as the Rust parser
records it).

It is a **flat arena**: one `Vec[Node]`, children are indices, `next` links
siblings, and text is a slice of the source. No node is heap-allocated
individually, which is what makes the tree cheap to build and cheap for the
checker to walk later.

```
$ ./wparse flagship/json.wlel bench 20
20 passes, 79300 nodes, 14 ms, 53 MB/s
```

Every name in the tree is a **source slice** (`slice_start`/`slice_len` on the
node, `-1` meaning "a synthesized string"), so parsing an identifier,
parameter, field or variant allocates nothing. The first version copied each
name into a `String`; that was 2 MB/s on the Windows runner against 37 MB/s on
Linux, which is why the slice form exists.

The parity gate builds the parser, dumps the tree for every source in the
repository, and compares it line for line against a Rust-side dumper of
`wlel::ast` — 41 files, item order, node shapes, operator codes, type
spellings and literal spellings included.

### Usage

```sh
wparse <file>            # node count (or the parse error, with line:col)
wparse <file> dump       # canonical AST, one node per line
wparse <file> bench [n]  # n in-process passes, prints MB/s
```

## The checker

`checker.wlel` decides whether a program is well typed. It reads the parser's
node arena directly — no tree, just indices — and covers scopes and shadowing,
literal widths and suffixes, arithmetic and comparison with the mixed-width
rules, bitwise and unary operators, casts, calls and arity, struct fields
(including `p.x` on a `*S`), indexing (strings index to a byte), `let`,
assignment and `return` compatibility, `if`/`while` conditions, function
signatures, `match` arms with pattern binding, enum construction, generic
signatures with `T` substitution, and the `std::`/`sys::` prelude.

It reads `lib/std.wlel` the same way the Rust checker splices it, so
`std::` names resolve from real declarations rather than a hand-written list.

The parity gate asks both checkers about every source in the repository and
compares their first diagnostic, `file:line:col: message`, exactly. Programs
that need work the stage does not do yet are listed in `KNOWN_GAPS` with the
reason; that list is a ratchet — the test fails when an entry starts agreeing,
so a fixed gap cannot be forgotten. Twenty broken programs pin the
diagnostics themselves, not just the agreement.

```
$ ./wcheck flagship/json.wlel bench 20
20 passes, 79300 nodes, 30 ms, 24 MB/s
```

### Usage

```sh
wcheck <file>          # "ok", or the first diagnostic on stderr
wcheck <file> -v       # also print every declared signature
wcheck <file> bench 20 # 20 full checks in-process, prints MB/s
```

## The codegen

`codegen.wlel` turns a checked program into C. It reads the types the checker
left in the node arena (`Node.ty`), so a type comparison is an integer
comparison and no type is ever re-parsed from a spelling. The generator covers
the core language: functions with prototypes, `let`/assignment/`return`,
`if`/`else`/`while`/range `for`, calls, unary and binary operators, casts,
structs (declaration, literal, field access, `->` through a pointer), tagged
enums with a payload union, `match` as a value, arrays by literal, `new` and
`sizeof`, string concatenation and the `std::print*` family.

The gate is **behavioural**: `tests/codegen_parity_tests.rs` compiles the
generated C and compares stdout and the exit status with the Rust compiler's
build of the same source — a different but correct C is allowed to differ in
text, and only behaviour may not. A mutation check (turning `+` into `-`)
confirms the gate fails when it should.

Not yet covered: `defer`, `try` (`expr?`), array types `[T; N]`, `for-in`,
`impl` methods, statement-form `match`, and the arena statement.

## Why parity first

A self-hosted compiler is only useful if it accepts exactly the language the
Rust one accepts. Comparing token boundaries catches the hard half of a lexer
(where tokens begin and end) and comparing the class catches the other half
(what they are), so the parser can be built on a stream that is already known
to be identical.
