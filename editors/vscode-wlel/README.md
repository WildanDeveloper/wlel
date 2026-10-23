# Wlel for VS Code

Syntax highlighting and language-server support for the
[Wlel](https://github.com/WildanDeveloper/wlel) language (`.wlel` files).

Zero dependencies: the TextMate grammar is plain JSON and the language
client is a small JSON-RPC bridge over the `wlel lsp` process, so there is
no build step and no `node_modules` tree.

## What you get

- Syntax highlighting for declarations, types, builtins, `std::`/`sys::`
  paths, doc comments, all literal forms (decimal, hex, binary, octal, with
  width suffixes) and operators.
- Bracket matching, auto-closing pairs and brace-based indentation.
- **Diagnostics** — every syntax error in one pass, the first type error, and
  warnings (unused variable/parameter/import, unreachable code), published on
  open and on every edit.
- **Hover** — variable and field types, function and method signatures,
  struct/enum shapes, variant payloads.
- **Go to definition** — locals, functions, methods, structs, enums,
  variants, fields, and `use "file.wlel"` imports.
- **Completion** — locals in scope, top-level symbols, the spliced stdlib
  (`vec_new`, `push`, `Ok`, …), keywords and primitive types.

## Requirements

The `wlel` compiler on `PATH` (it provides `wlel lsp`). Prebuilt binaries are
on the [releases page](https://github.com/WildanDeveloper/wlel/releases).

## Settings

| Setting | Default | Meaning |
|---|---|---|
| `wlel.serverPath` | `wlel` | Path to the compiler binary |
| `wlel.serverArgs` | `["lsp"]` | Arguments for the server |
| `wlel.trace.server` | `off` | `messages`/`verbose` log the JSON-RPC traffic to the **Wlel** output channel |

## Installing from source

```bash
git clone https://github.com/WildanDeveloper/wlel
cp -r wlel/editors/vscode-wlel ~/.vscode/extensions/wlel-1.1.0
# reload the window; open any .wlel file
```

## Notes

- The stdlib is embedded in the compiler, so std symbols resolve without a
  `std` source file on disk; go-to-definition on a std symbol reports a
  `wlel-std:` URI that has no file behind it.
- Unsaved buffers analyze fine, but `use "file.wlel"` imports resolve from
  disk, so save before relying on cross-file go-to-definition.
- If the server cannot start, the **Wlel** output channel says why and the
  extension retries after the next edit.
