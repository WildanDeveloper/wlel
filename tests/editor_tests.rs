//! Editor support: the VS Code extension and the tree-sitter grammar.
//!
//! These artifacts are JSON, JavaScript and generated C, none of which the
//! Rust compiler can check, so the tests validate them the way a consumer
//! would: parse the JSON, compile the grammar, and parse every `.wlel` file
//! in the repository with the generated parser.
//!
//! The crate itself has no dependencies, so the small JSON reader below keeps
//! it that way. `node` and `tree-sitter` are optional: when a tool is missing
//! the test prints why and returns instead of failing, so a contributor
//! without npm still gets a green `cargo test`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn vscode() -> PathBuf {
    root().join("editors/vscode-wlel")
}

fn tree_sitter() -> PathBuf {
    root().join("editors/tree-sitter-wlel")
}

/// `node --version`, `cc --version`, `tree-sitter --version` all exist. This
/// is a probe, not a guarantee: a tool that answers is used.
fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Every `.wlel` file in the repository, skipping build output, the git
/// directory and editor bundles (they contain no sources of their own).
fn wlel_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if matches!(
                    name.as_ref(),
                    "target" | ".git" | "node_modules" | ".wlel" | "build"
                ) {
                    continue;
                }
                walk(&path, out);
            } else if name.ends_with(".wlel") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root(), &mut out);
    out.sort();
    out
}

// ---- a minimal JSON reader (no dependencies) ------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    fn as_object(&self) -> Option<&Vec<(String, Json)>> {
        match self {
            Json::Object(fields) => Some(fields),
            _ => None,
        }
    }

    fn is_string(&self, expected: &str) -> bool {
        self.as_str() == Some(expected)
    }
}

fn parse_json(text: &str) -> Result<Json, String> {
    let bytes = text.as_bytes();
    let mut pos = 0;
    let value = json_value(bytes, &mut pos)?;
    json_ws(bytes, &mut pos);
    if pos != bytes.len() {
        return Err(format!("trailing input at byte {pos}"));
    }
    Ok(value)
}

fn json_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }
}

fn json_value(bytes: &[u8], pos: &mut usize) -> Result<Json, String> {
    json_ws(bytes, pos);
    let Some(&byte) = bytes.get(*pos) else {
        return Err("unexpected end of input".into());
    };
    match byte {
        b'{' => {
            *pos += 1;
            let mut fields = Vec::new();
            json_ws(bytes, pos);
            if bytes.get(*pos) == Some(&b'}') {
                *pos += 1;
                return Ok(Json::Object(fields));
            }
            loop {
                json_ws(bytes, pos);
                let key = match json_value(bytes, pos)? {
                    Json::String(s) => s,
                    _ => return Err(format!("object key at byte {pos} is not a string")),
                };
                json_ws(bytes, pos);
                if bytes.get(*pos) != Some(&b':') {
                    return Err(format!("expected ':' at byte {pos}"));
                }
                *pos += 1;
                let value = json_value(bytes, pos)?;
                fields.push((key, value));
                json_ws(bytes, pos);
                match bytes.get(*pos) {
                    Some(b',') => *pos += 1,
                    Some(b'}') => {
                        *pos += 1;
                        return Ok(Json::Object(fields));
                    }
                    _ => return Err(format!("expected ',' or '}}' at byte {pos}")),
                }
            }
        }
        b'[' => {
            *pos += 1;
            let mut items = Vec::new();
            json_ws(bytes, pos);
            if bytes.get(*pos) == Some(&b']') {
                *pos += 1;
                return Ok(Json::Array(items));
            }
            loop {
                items.push(json_value(bytes, pos)?);
                json_ws(bytes, pos);
                match bytes.get(*pos) {
                    Some(b',') => *pos += 1,
                    Some(b']') => {
                        *pos += 1;
                        return Ok(Json::Array(items));
                    }
                    _ => return Err(format!("expected ',' or ']' at byte {pos}")),
                }
            }
        }
        b'"' => {
            *pos += 1;
            let mut out = String::new();
            loop {
                let Some(&c) = bytes.get(*pos) else {
                    return Err("unterminated string".into());
                };
                *pos += 1;
                match c {
                    b'"' => return Ok(Json::String(out)),
                    b'\\' => {
                        let Some(&esc) = bytes.get(*pos) else {
                            return Err("unterminated escape".into());
                        };
                        *pos += 1;
                        out.push(match esc {
                            b'"' => '"',
                            b'\\' => '\\',
                            b'/' => '/',
                            b'b' => '\u{8}',
                            b'f' => '\u{c}',
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'u' => {
                                let hex = std::str::from_utf8(
                                    bytes.get(*pos..*pos + 4).ok_or("short \\u escape")?,
                                )
                                .map_err(|e| e.to_string())?;
                                *pos += 4;
                                char::from_u32(
                                    u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?,
                                )
                                .ok_or("invalid \\u escape")?
                            }
                            other => return Err(format!("unknown escape \\{}", other as char)),
                        });
                    }
                    _ => {
                        // UTF-8 continuation bytes pass through
                        let start = *pos - 1;
                        let len = if c & 0x80 == 0 {
                            1
                        } else if c & 0xE0 == 0xC0 {
                            2
                        } else if c & 0xF0 == 0xE0 {
                            3
                        } else {
                            4
                        };
                        let end = (start + len).min(bytes.len());
                        out.push_str(
                            std::str::from_utf8(&bytes[start..end]).map_err(|e| e.to_string())?,
                        );
                        *pos = end;
                    }
                }
            }
        }
        b't' if bytes.get(*pos..*pos + 4) == Some(b"true") => {
            *pos += 4;
            Ok(Json::Bool(true))
        }
        b'f' if bytes.get(*pos..*pos + 5) == Some(b"false") => {
            *pos += 5;
            Ok(Json::Bool(false))
        }
        b'n' if bytes.get(*pos..*pos + 4) == Some(b"null") => {
            *pos += 4;
            Ok(Json::Null)
        }
        b'-' | b'0'..=b'9' => {
            let start = *pos;
            while *pos < bytes.len()
                && matches!(bytes[*pos], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
            {
                *pos += 1;
            }
            let text = std::str::from_utf8(&bytes[start..*pos]).map_err(|e| e.to_string())?;
            text.parse::<f64>()
                .map(Json::Number)
                .map_err(|_| format!("invalid number at byte {start}: {text}"))
        }
        other => Err(format!("unexpected byte {} at {pos}", other as char)),
    }
}

fn read_json(path: &Path) -> Json {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    parse_json(&text).unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

// ---- VS Code extension ----------------------------------------------------

#[test]
fn vscode_manifest_is_well_formed() {
    let dir = vscode();
    let json = read_json(&dir.join("package.json"));

    assert!(json.get("name").is_some_and(|v| v.is_string("wlel")));
    assert!(json.get("license").is_some_and(|v| v.is_string("Apache-2.0")));
    assert!(
        json.get("publisher").is_some_and(|v| v.is_string("WildanDeveloper")),
        "the marketplace publisher is part of the extension id"
    );
    // plain JavaScript: no build step, so no scripts and no dependencies
    assert!(
        json.get("scripts").is_none(),
        "extension must not need a build step"
    );
    assert!(
        json.get("devDependencies").is_none(),
        "extension must not need node_modules"
    );

    let main = json.get("main").and_then(Json::as_str).expect("main entry");
    assert!(main.starts_with("./"), "main must be a relative path: {main}");
    assert!(
        dir.join(main.trim_start_matches("./")).is_file(),
        "main points at a file that does not exist: {main}"
    );

    let contributes = json.get("contributes").expect("contributes");
    let languages = contributes
        .get("languages")
        .and_then(Json::as_array)
        .expect("contributes.languages");
    assert_eq!(languages.len(), 1);
    assert_eq!(
        languages[0].get("id").and_then(Json::as_str),
        Some("wlel")
    );
    let extensions = languages[0]
        .get("extensions")
        .and_then(Json::as_array)
        .expect("extensions");
    assert_eq!(extensions.len(), 1);
    assert!(
        extensions[0].is_string(".wlel"),
        "the language's extension is .wlel (not .wl, which Linguist gives to Wolfram)"
    );

    let grammars = contributes
        .get("grammars")
        .and_then(Json::as_array)
        .expect("contributes.grammars");
    assert_eq!(
        grammars[0].get("scopeName").and_then(Json::as_str),
        Some("source.wlel")
    );
    let grammar_path = grammars[0]
        .get("path")
        .and_then(Json::as_str)
        .expect("grammar path");
    assert!(
        dir.join(grammar_path.trim_start_matches("./")).is_file(),
        "grammar file is missing: {grammar_path}"
    );

    // every file the manifest points at must exist
    if let Some(rel) = languages[0].get("configuration").and_then(Json::as_str) {
        assert!(
            dir.join(rel.trim_start_matches("./")).is_file(),
            "language configuration is missing: {rel}"
        );
    }
}

#[test]
fn vscode_textmate_grammar_is_consistent() {
    let json = read_json(&vscode().join("syntaxes/wlel.tmLanguage.json"));

    assert!(json.get("scopeName").is_some_and(|v| v.is_string("source.wlel")));
    let file_types = json.get("fileTypes").and_then(Json::as_array).expect("fileTypes");
    assert!(file_types.iter().any(|v| v.is_string("wlel")));

    let repository = json
        .get("repository")
        .and_then(Json::as_object)
        .expect("repository");
    let patterns = json.get("patterns").and_then(Json::as_array).expect("patterns");
    assert!(!patterns.is_empty());

    fn check_includes(node: &Json, repository: &[(String, Json)]) {
        match node {
            Json::Object(fields) => {
                for (key, value) in fields {
                    if key == "include" {
                        if let Some(name) = value.as_str().and_then(|s| s.strip_prefix('#')) {
                            assert!(
                                repository.iter().any(|(rule, _)| rule == name),
                                "include #{name} has no rule"
                            );
                        }
                    }
                    check_includes(value, repository);
                }
            }
            Json::Array(items) => {
                for item in items {
                    check_includes(item, repository);
                }
            }
            _ => {}
        }
    }
    for pattern in patterns {
        check_includes(pattern, repository);
    }
    for (_, rule) in repository {
        check_includes(rule, repository);
    }
}

#[test]
fn vscode_language_configuration_is_well_formed() {
    let json = read_json(&vscode().join("language-configuration.json"));

    assert!(
        json.get("comments")
            .and_then(|c| c.get("lineComment"))
            .is_some_and(|v| v.is_string("//"))
    );
    let brackets = json.get("brackets").and_then(Json::as_array).expect("brackets");
    assert!(brackets.iter().any(|pair| {
        pair.as_array().is_some_and(|p| {
            p.len() == 2 && p[0].is_string("{") && p[1].is_string("}")
        })
    }));
    assert!(json
        .get("indentationRules")
        .and_then(|r| r.get("increaseIndentPattern"))
        .and_then(Json::as_str)
        .is_some());
}

#[test]
fn vscode_extension_javascript_parses() {
    if !have("node") {
        eprintln!("skipping: node is not on PATH");
        return;
    }
    for script in ["src/extension.js", "test/bridge.js"] {
        let path = vscode().join(script);
        assert!(path.is_file(), "{script} is missing");
        let out = Command::new("node")
            .args(["--check", path.to_str().unwrap()])
            .output()
            .expect("run node --check");
        assert!(
            out.status.success(),
            "{script} does not parse:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Drive the real extension against the real language server: the only test
/// that proves the two halves talk to each other. Asserts on hover,
/// go-to-definition, completion and diagnostics.
#[test]
fn vscode_extension_talks_to_the_language_server() {
    if !have("node") {
        eprintln!("skipping: node is not on PATH");
        return;
    }
    let harness = vscode().join("test/bridge.js");
    assert!(harness.is_file(), "the extension test harness is missing");
    let out = Command::new("node")
        .arg(&harness)
        .env("WLEL_BIN", env!("CARGO_BIN_EXE_wlel"))
        .output()
        .expect("run the extension harness");
    assert!(
        out.status.success(),
        "the extension could not talk to `wlel lsp`:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---- tree-sitter grammar --------------------------------------------------

#[test]
fn tree_sitter_metadata_is_well_formed() {
    let dir = tree_sitter();

    let pkg = read_json(&dir.join("package.json"));
    assert!(pkg.get("name").is_some_and(|v| v.is_string("tree-sitter-wlel")));
    assert!(pkg.get("license").is_some_and(|v| v.is_string("Apache-2.0")));
    let grammar = pkg
        .get("tree-sitter")
        .and_then(Json::as_array)
        .and_then(|g| g.first())
        .expect("package.json tree-sitter entry");
    assert!(grammar.get("scope").is_some_and(|v| v.is_string("source.wlel")));
    assert!(grammar
        .get("file-types")
        .and_then(Json::as_array)
        .is_some_and(|f| f.iter().any(|v| v.is_string("wlel"))));

    let config = read_json(&dir.join("tree-sitter.json"));
    let grammars = config
        .get("grammars")
        .and_then(Json::as_array)
        .expect("tree-sitter.json grammars");
    assert!(grammars[0].get("name").is_some_and(|v| v.is_string("wlel")));
    assert!(grammars[0]
        .get("scope")
        .is_some_and(|v| v.is_string("source.wlel")));

    // the generated parser and its metadata are committed, so consumers only
    // need a C compiler
    for generated in ["src/parser.c", "src/grammar.json", "src/node-types.json"] {
        assert!(dir.join(generated).is_file(), "{generated} is not committed");
    }
    for query in ["queries/highlights.scm", "queries/locals.scm"] {
        assert!(dir.join(query).is_file(), "{query} is missing");
    }
    let corpus = fs::read_dir(dir.join("test/corpus")).expect("test/corpus");
    let mut cases = 0;
    for entry in corpus.flatten() {
        let path = entry.path();
        assert_eq!(
            path.extension().and_then(|e| e.to_str()),
            Some("txt"),
            "corpus files must be .txt: {}",
            path.display()
        );
        cases += 1;
    }
    assert!(cases >= 3, "expected the grammar corpus to have tests");
}

#[test]
fn tree_sitter_generated_sources_are_current() {
    if !have("tree-sitter") {
        eprintln!("skipping: tree-sitter CLI is not on PATH");
        return;
    }
    let dir = tree_sitter();

    // regenerate in a scratch copy so the working tree is never touched
    let scratch = std::env::temp_dir().join(format!("wlel_tsgen_{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).expect("mkdir");
    for file in ["grammar.js", "package.json", "tree-sitter.json"] {
        fs::copy(dir.join(file), scratch.join(file))
            .unwrap_or_else(|e| panic!("copy {file}: {e}"));
    }

    let out = Command::new("tree-sitter")
        .arg("generate")
        .current_dir(&scratch)
        .output()
        .expect("run tree-sitter generate");

    let mut stale = Vec::new();
    if out.status.success() {
        for generated in ["src/parser.c", "src/grammar.json", "src/node-types.json"] {
            let fresh = fs::read(scratch.join(generated)).unwrap_or_default();
            let committed = fs::read(dir.join(generated)).unwrap_or_default();
            if fresh != committed {
                stale.push(generated);
            }
        }
    }
    let _ = fs::remove_dir_all(&scratch);

    assert!(
        out.status.success(),
        "tree-sitter generate failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stale.is_empty(),
        "src/ is out of date with grammar.js — run `tree-sitter generate` in \
         editors/tree-sitter-wlel and commit: {}",
        stale.join(", ")
    );
}

#[test]
fn tree_sitter_parser_compiles_as_c() {
    let compiler = if have("cc") {
        "cc"
    } else if have("gcc") {
        "gcc"
    } else if have("clang") {
        "clang"
    } else {
        eprintln!("skipping: no C compiler on PATH");
        return;
    };

    let dir = tree_sitter();
    let out_dir = std::env::temp_dir().join(format!("wlel_ts_{}", std::process::id()));
    let _ = fs::remove_dir_all(&out_dir);
    fs::create_dir_all(&out_dir).expect("mkdir");

    let obj = out_dir.join("parser.o");
    let out = Command::new(compiler)
        .args(["-std=c11", "-fPIC", "-c"])
        .arg(dir.join("src/parser.c"))
        .arg("-I")
        .arg(dir.join("src"))
        .arg("-o")
        .arg(&obj)
        .output()
        .expect("run the C compiler");
    let _ = fs::remove_dir_all(&out_dir);
    assert!(
        out.status.success(),
        "src/parser.c does not compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn tree_sitter_parses_every_example_without_errors() {
    if !have("tree-sitter") {
        eprintln!("skipping: tree-sitter CLI is not on PATH");
        return;
    }
    let sources = wlel_sources();
    assert!(
        sources.len() >= 25,
        "expected the examples and flagships to be present, found {}",
        sources.len()
    );

    let mut failures = Vec::new();
    for source in &sources {
        let out = Command::new("tree-sitter")
            .args(["parse", source.to_str().unwrap()])
            .current_dir(tree_sitter())
            .output()
            .expect("run tree-sitter parse");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{stdout}{stderr}");
        if !out.status.success() || combined.contains("ERROR") || combined.contains("MISSING") {
            let detail: Vec<&str> = combined
                .lines()
                .filter(|l| l.contains("ERROR") || l.contains("MISSING"))
                .take(3)
                .collect();
            failures.push(format!("{}\n{}", source.display(), detail.join("\n")));
        }
    }
    assert!(
        failures.is_empty(),
        "the tree-sitter grammar rejected {} of {} files:\n{}",
        failures.len(),
        sources.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn tree_sitter_corpus_tests_pass() {
    if !have("tree-sitter") {
        eprintln!("skipping: tree-sitter CLI is not on PATH");
        return;
    }
    let out = Command::new("tree-sitter")
        .arg("test")
        .current_dir(tree_sitter())
        .output()
        .expect("run tree-sitter test");
    assert!(
        out.status.success(),
        "tree-sitter corpus tests failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn tree_sitter_queries_compile() {
    if !have("tree-sitter") {
        eprintln!("skipping: tree-sitter CLI is not on PATH");
        return;
    }
    let dir = tree_sitter();
    let sample = root().join("examples/kitchen.wlel");
    assert!(sample.is_file(), "examples/kitchen.wlel is missing");

    for query in ["queries/highlights.scm", "queries/locals.scm"] {
        let out = Command::new("tree-sitter")
            .args(["query", query, sample.to_str().unwrap()])
            .current_dir(&dir)
            .output()
            .expect("run tree-sitter query");
        assert!(
            out.status.success(),
            "{query} does not compile:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

// ---- both ----------------------------------------------------------------

#[test]
fn editor_bundles_carry_the_license() {
    for dir in [vscode(), tree_sitter(), root().join("editors")] {
        for file in ["LICENSE", "NOTICE", "README.md"] {
            if dir.ends_with("editors") && file != "README.md" {
                // the shared LICENSE/NOTICE live in each bundle, and the
                // repository root covers the directory itself
                continue;
            }
            assert!(dir.join(file).is_file(), "{} has no {file}", dir.display());
        }
    }
    for file in ["LICENSE", "NOTICE"] {
        assert!(root().join(file).is_file(), "the repository has no {file}");
    }
}
