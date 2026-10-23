# tree-sitter-wlel

A [tree-sitter](https://tree-sitter.github.io/tree-sitter/) grammar for
[Wlel](https://github.com/WildanDeveloper/wlel), the arena-first systems
language (`.wlel` files).

The grammar mirrors the reference parser in
[`docs/spec.md` §16](https://github.com/WildanDeveloper/wlel/blob/main/docs/spec.md#16-grammar)
and parses every file in `examples/` and `flagship/` with zero `ERROR` or
`MISSING` nodes.

## Use it

### Neovim (nvim-treesitter)

```lua
local parser_config = require('nvim-treesitter.parsers').get_parser_configs()
parser_config.wlel = {
  install_info = {
    url = 'https://github.com/WildanDeveloper/wlel',
    files = { 'editors/tree-sitter-wlel/src/parser.c' },
    location = 'editors/tree-sitter-wlel',
  },
  filetype = 'wlel',
}
```

Then `:TSInstall wlel`, and copy `queries/highlights.scm` and
`queries/locals.scm` into `queries/wlel/`.

### Helix

Add to `languages.toml`:

```toml
[[language]]
name = "wlel"
scope = "source.wlel"
file-types = ["wlel"]
comment-token = "//"
indent = { tab-width = 4, unit = "    " }
grammar = "wlel"

[[grammar]]
name = "wlel"
source = { git = "https://github.com/WildanDeveloper/wlel", subpath = "editors/tree-sitter-wlel" }
```

Then `hx --grammar fetch && hx --grammar build`, and copy the two query
files into `runtime/queries/wlel/`.

### Emacs (tree-sitter)

`tree-sitter-langs`-style build:

```elisp
(add-to-list 'treesit-language-source-alist
             '(wlel "https://github.com/WildanDeveloper/wlel"
                    "main" "editors/tree-sitter-wlel/src"))
(treesit-install-language-grammar 'wlel)
```

### Node

```js
const Parser = require('tree-sitter');
const Wlel = require('tree-sitter-wlel');

const parser = new Parser();
parser.setLanguage(Wlel);
console.log(parser.parse('fn main() -> int { return 0; }').rootNode.toString());
```

## Build from source

```sh
cd editors/tree-sitter-wlel
tree-sitter generate      # grammar.js -> src/parser.c
tree-sitter test          # corpus tests under test/corpus/
tree-sitter parse ../../examples/kitchen.wlel
```

`src/` is checked in, so consumers only need a C compiler.

## Layout

| Path | What |
|---|---|
| `grammar.js` | The grammar (single source of truth) |
| `src/parser.c` | Generated parser (committed) |
| `src/node-types.json` | Generated node metadata (committed) |
| `queries/highlights.scm` | Syntax highlighting captures |
| `queries/locals.scm` | Scopes, definitions and references |
| `test/corpus/*.txt` | Grammar tests run by `tree-sitter test` |

## Notes on the grammar

Two places need care, both documented at the top of `grammar.js`:

- `Name { field: value }` is a struct literal, so an expression at the end of
  a condition could swallow the `{` that opens the block. Conditions and
  operands use a rule that excludes struct literals; parenthesize a struct
  literal if it really is a condition.
- `a[i]` and `vec_new[int](...)` share the `name [` prefix. The bracket
  content is parsed once and only resolved into an index or type arguments
  after the closing `]`.

## License

Apache-2.0, same as the compiler. Generated C (`src/`) is part of the
grammar and carries the same license.
