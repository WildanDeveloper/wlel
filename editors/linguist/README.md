# GitHub Linguist registration for Wlel

GitHub only knows a language after [github/linguist] ships an entry for it.
This directory is the registration kit: everything the Linguist pull request
needs, already reviewed.

Until that PR merges, `.gitattributes` in the repository root overrides the
language for this repo (`*.wlel linguist-language=Wlel`), which is enough for
the language bar and for `linguist-detectable`.

[github/linguist]: https://github.com/github/linguist

## What is here

| File | Destination in the Linguist PR |
|---|---|
| `languages.yml` | append to `lib/linguist/languages.yml` |
| `grammars.yml` | append under `grammars:` in `grammars.yml` |
| `prepare.sh` | stages the samples and the two YAML fragments |

Linguist highlights code samples with a **TextMate** grammar, not a
tree-sitter one, so the vendored grammar is `editors/vscode-wlel`
(`syntaxes/wlel.tmLanguage.json`, scope `source.wlel`). The tree-sitter
grammar in `editors/tree-sitter-wlel` is used by editors, not by Linguist.

## The pull request

1. **Fork** `github/linguist` and branch from `master`.

2. **Add the grammar as a submodule** (this is the VS Code extension, which
   contains the TextMate grammar):

   ```sh
   git submodule add https://github.com/WildanDeveloper/wlel \
       vendor/grammars/vscode-wlel
   ```

3. **Register the grammar** in `grammars.yml` — append `grammars.yml` from
   this directory (it maps the submodule path to `source.wlel`).

4. **Add the language** to `lib/linguist/languages.yml` — append
   `languages.yml` from this directory. It deliberately leaves out
   `language_id`: Linguist assigns a fresh one (a random 9–10 digit number)
   when the PR is accepted, and many registered languages have no id yet.

5. **Add samples.** Run the staging script and copy the result:

   ```sh
   ./editors/linguist/prepare.sh /tmp/linguist-wlel
   cp /tmp/linguist-wlel/samples/Wlel/* <linguist>/samples/Wlel/
   ```

   Samples must be real, compiling programs that exercise the syntax; they
   feed both detection and the highlighting check. The script uses the
   repository's own examples so they cannot drift from the compiler.

6. **Run the checks:**

   ```sh
   bundle install
   bundle exec rake test
   bundle exec github-linguist --breakdown samples/Wlel
   ```

7. **Open the PR** titled `Add Wlel`, link
   <https://github.com/WildanDeveloper/wlel>, and say what the language is in
   two or three sentences. Registration usually takes a few weeks and one or
   two review rounds.

## Checklist before submitting

- [ ] The language has a public home page or repository.
- [ ] A handful of public repositories use `.wlel` files.
- [ ] The extension `.wlel` is not claimed by another language.
- [ ] `wl` is deliberately absent: it belongs to Wolfram Language, and Wlel
      moved off it before 1.0.
- [ ] A TextMate grammar with scope `source.wlel` exists
      (`editors/vscode-wlel`).
- [ ] Samples compile with the released compiler.
