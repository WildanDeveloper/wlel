# Self-hosting

The Wlel compiler is written in Rust. This directory is the Wlel rewrite, one
stage at a time, each stage gated by a parity test against the Rust
implementation before the next one starts.

| Stage | Source | Gate |
|---|---|---|
| lexer | `lexer.wlel` | `tests/selfhost_tests.rs` — token-for-token parity on every `.wlel` file in the repository |
| parser | `parser.wlel` | `tests/parser_parity_tests.rs` — AST dump parity on every `.wlel` file |
| checker | planned | diagnostics parity |
| codegen | planned | generated C diff parity |

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
$ ./wparse flagship/json.wlel bench 50
20 passes, 79300 nodes, 20 ms, 37 MB/s
```

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

## Why parity first

A self-hosted compiler is only useful if it accepts exactly the language the
Rust one accepts. Comparing token boundaries catches the hard half of a lexer
(where tokens begin and end) and comparing the class catches the other half
(what they are), so the parser can be built on a stream that is already known
to be identical.
