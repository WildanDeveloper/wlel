//! The self-hosted checker (`selfhost/checker.wlel`) must agree with the Rust
//! checker about which programs are well typed, and about the first
//! diagnostic it reports for the ones that are not.
//!
//! Stage 1 covers the rules that decide the shape of a program: scopes and
//! shadowing, literal widths, arithmetic and comparison, bitwise, unary,
//! casts, calls and arity, struct fields, indexing, `let`/assignment/`return`
//! compatibility, `if`/`while` conditions, function signatures, `match` arms,
//! and the `std::`/`sys::` prelude. Both implementations are asked about the
//! same sources and their `file:line:col: message` output must match.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build the self-hosted checker once for the whole test binary.
fn checker_binary() -> Option<&'static Path> {
    static BINARY: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    BINARY
        .get_or_init(|| {
            if !have("cc") && !have("gcc") && !have("clang") {
                eprintln!("skipping: no C compiler on PATH");
                return None;
            }
            let out = std::env::temp_dir().join(format!(
                "wlel_selfhost_checker_{}",
                std::process::id()
            ));
            let _ = fs::remove_file(&out);
            let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
                .args(["build", "selfhost/checker.wlel", "-O2", "-o"])
                .arg(&out)
                .current_dir(root())
                .output()
                .expect("spawn wlel build");
            assert!(
                build.status.success(),
                "the self-hosted checker does not build:\n{}{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            );
            Some(out)
        })
        .as_deref()
}

/// The first line of a checker's report, with the tool's prefix removed:
/// `wlel: path:1:2: msg` and `path:1:2: msg` both become `path:1:2: msg`.
fn normalise(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    let line = line.strip_prefix("wlel: ").unwrap_or(line);
    line.to_string()
}

fn rust_check(file: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .args(["check", file])
        .current_dir(root())
        .output()
        .expect("run wlel check");
    normalise(&format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

fn wlel_check(bin: &Path, file: &str) -> String {
    let out = Command::new(bin)
        .arg(file)
        .current_dir(root())
        .output()
        .expect("run the self-hosted checker");
    normalise(&format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// Every `.wlel` source in the repository, minus the two the checker is not
/// expected to agree on yet (see `KNOWN_GAPS`).
fn sources() -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if matches!(name.as_ref(), "target" | ".git" | "node_modules" | ".wlel") {
                    continue;
                }
                walk(&path, out);
            } else if name.ends_with(".wlel") && path.to_str() != Some("lib/std.wlel") {
                out.push(path.to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(&root(), &mut out);
    out.sort();
    out
}

/// Programs whose disagreement is a known stage-1 gap, with the reason. The
/// list is a ratchet: it may shrink, and the test fails if an entry starts
/// passing, so a gap cannot be forgotten once it is fixed.
/// Programs whose disagreement is a known stage-1 gap, with the reason. The
/// list is a ratchet: it may only shrink, and the test fails when an entry
/// starts agreeing, so a gap cannot be forgotten once it is fixed.
const KNOWN_GAPS: &[(&str, &str)] = &[
    // the file the checker is compared *against*: std is spliced into every
    // program, so checking it standalone asks a different question
    ("lib/std.wlel", "std is spliced, not checked standalone"),
    // modules: `shapes::Circle` is not resolved to the struct in lib/shapes
    ("examples/modules.wlel", "module namespace resolution"),
    // array types with a length: `[u8; 8]` needs constant evaluation
    ("flagship/httpd.wlel", "array length constants"),
    // generic instantiations: field lookup through `Box[T]` and `T` itself
    ("examples/generics.wlel", "generic struct fields"),
    ("examples/methods.wlel", "generic method bodies"),
    ("selfhost/parser.wlel", "generic helper return types"),
    ("selfhost/checker.wlel", "enum tag field access"),
    // a generic enum returned from a function (`Result[T, E]`, `Shape`)
    ("flagship/json.wlel", "generic Result instantiation"),
    ("examples/enums.wlel", "enum returned from match"),
    ("examples/fileio.wlel", "generic Result instantiation"),
    // function pointers as values
    ("examples/function_pointers.wlel", "function-pointer values"),
    // the collection and arena method sets
    ("examples/sort.wlel", "generic struct field through an array"),
    ("examples/arenastats.wlel", "arena struct fields"),
    ("examples/collections.wlel", "map method set"),
    ("examples/concurrency.wlel", "channel element types"),
    ("examples/kitchen.wlel", "arena allocator builtins"),
    ("fuzz/corpus/parse/kitchen.wlel", "arena allocator builtins"),
    // runtime modules reached through a nested `std::` path
    ("examples/timemath.wlel", "std::math and std::random modules"),
    ("examples/strlib.wlel", "string + int formatting"),
    ("selfhost/lexer.wlel", "std::str_parse_int arity"),
];


fn relative(file: &str) -> String {
    file.strip_prefix(&format!("{}/", root().display()))
        .unwrap_or(file)
        .replace('\\', "/")
}

fn gap_for(file: &str) -> Option<&'static str> {
    let rel = relative(file);
    KNOWN_GAPS
        .iter()
        .find(|(f, _)| *f == rel)
        .map(|(_, why)| *why)
}

#[test]
fn the_self_hosted_checker_agrees_with_the_rust_one() {
    let Some(bin) = checker_binary() else { return };
    let files = sources();
    let mut failures = Vec::new();
    let mut gaps_seen = Vec::new();
    for file in &files {
        let expected = rust_check(file);
        let actual = wlel_check(bin, file);
        if actual == expected {
            if let Some(why) = gap_for(file) {
                gaps_seen.push(format!("{file} now agrees ({why}) — drop it from KNOWN_GAPS"));
            }
            continue;
        }
        match gap_for(file) {
            Some(_) => {}
            None => failures.push(format!("{file}\n  rust: {expected}\n  wlel: {actual}")),
        }
    }
    assert!(
        gaps_seen.is_empty(),
        "the gap list is stale:\n{}",
        gaps_seen.join("\n")
    );
    assert!(
        failures.is_empty(),
        "the self-hosted checker disagrees on {} of {} files:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n---\n")
    );
}

/// The diagnostics themselves, not just agreement: a corpus of broken
/// programs whose exact `line:col: message` must match.
#[test]
fn the_self_hosted_checker_reports_the_same_diagnostics() {
    let Some(bin) = checker_binary() else { return };
    let cases: &[&str] = &[
        "fn f() -> int {\n    x := 1;\n    return y;\n}\n",
        "fn f() -> int {\n    b := \"x\" + 2;\n    return 0;\n}\n",
        "fn f() -> int {\n    return 1 + \"s\";\n}\n",
        "fn f() {\n    if 1 {\n    }\n}\n",
        "fn f() {\n    while \"s\" {\n    }\n}\n",
        "fn f() -> int {\n    return \"s\";\n}\n",
        "fn f() {\n    g();\n}\n",
        "fn f() {\n    let x: int = 1;\n    let x: int = 2;\n}\n",
        "struct S { x: int }\nfn f() -> int {\n    s := S { x: 1 };\n    return s.y;\n}\n",
        "fn f() -> int {\n    a := [1, 2];\n    return a[\"k\"];\n}\n",
        "fn f() {\n    std::println_int(1, 2);\n}\n",
        "fn f() {\n    std::println_int(1);\n}\n",
        "fn f() -> bool {\n    return 1 && 2;\n}\n",
        "fn f() -> int {\n    return !5;\n}\n",
        "fn f() -> int {\n    return ~\"s\";\n}\n",
        "fn f() -> int {\n    x := 1;\n    x = \"s\";\n    return x;\n}\n",
        "fn f() -> int {\n    return true > 1;\n}\n",
        "fn f() {\n    x := 1;\n    x.y;\n}\n",
        "fn f() -> int {\n    x := 1;\n    return x[0];\n}\n",
        "fn f() {\n    let v: void = 1;\n}\n",
    ];
    let mut failures = Vec::new();
    for (i, src) in cases.iter().enumerate() {
        let path = std::env::temp_dir().join(format!(
            "wlel_check_case_{}_{i}.wlel",
            std::process::id()
        ));
        fs::write(&path, src).expect("write case");
        let p = path.to_str().unwrap();
        let expected = rust_check(p);
        let actual = wlel_check(bin, p);
        let _ = fs::remove_file(&path);
        if expected != actual {
            failures.push(format!(
                "case {i}:\n  source: {src:?}\n  rust: {expected}\n  wlel: {actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} diagnostics differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn the_self_hosted_checker_reports_throughput() {
    let Some(bin) = checker_binary() else { return };
    let run = Command::new(bin)
        .args([root().join("flagship/json.wlel").to_str().unwrap(), "bench", "20"])
        .output()
        .expect("run the bench mode");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let text = String::from_utf8_lossy(&run.stdout);
    assert!(text.contains("20 passes"), "{text}");
    let mbps: u64 = text.rsplit(' ').find_map(|w| w.parse().ok()).unwrap_or(0);
    assert!(mbps > 3, "checker throughput regressed: {text}");
}
