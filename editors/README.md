# Wlel editor support

Three independent pieces, all Apache-2.0:

| Directory | What it is |
|---|---|
| [`vscode-wlel`](vscode-wlel) | VS Code extension: TextMate grammar + a zero-dependency client for `wlel lsp` |
| [`tree-sitter-wlel`](tree-sitter-wlel) | tree-sitter grammar `source.wlel`, with highlight/local queries and corpus tests |
| [`linguist`](linguist) | GitHub Linguist registration kit (YAML fragments, sample staging, PR checklist) |

None of them needs a build step to be used: the VS Code extension is plain
JavaScript, and the tree-sitter grammar ships its generated `src/`.

## What gets checked

`tests/editor_tests.rs` in the repository root validates all of it:

- manifest, TextMate grammar and language-configuration JSON parse, every
  `include`/path resolves, and the extension has no scripts or dependencies;
- `node --check` passes on `src/extension.js` and `test/bridge.js`;
- the extension is loaded against a stub `vscode` module and driven against
  the real `wlel lsp` binary — hover, go-to-definition, completion and
  diagnostics;
- `src/parser.c` compiles as C11, and regenerating the grammar reproduces the
  committed `src/` byte-for-byte;
- `tree-sitter test` passes and both query files compile;
- every `.wlel` file in the repository parses with zero `ERROR`/`MISSING`.

Tests that need `node`, `cc` or `tree-sitter` print a skip line when the tool
is missing, so `cargo test` stays green on a bare machine. CI runs them with
the tools installed (the `editors` job in `.github/workflows/ci.yml`).

## Regenerating the tree-sitter parser

```sh
cd editors/tree-sitter-wlel
npm install -g tree-sitter-cli   # once
tree-sitter generate
tree-sitter test
```

`grammar.js` is the source of truth; `src/` is committed so consumers only
need a C compiler. The editor test fails if the two disagree.
