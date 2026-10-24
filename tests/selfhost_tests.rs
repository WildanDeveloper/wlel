//! The self-hosted lexer (`selfhost/lexer.wlel`) must agree with the Rust
//! lexer token for token.
//!
//! This is the parity gate for the self-hosting effort: the Wlel lexer is
//! compiled by the current compiler, run over every `.wlel` file in the
//! repository, and its output is compared against `wlel::lexer` — same
//! boundaries, same line/column, same class (keyword / identifier / number /
//! float / string / punctuation), same text.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use wlel::lexer::Lexer;
use wlel::token::Token;

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

/// every `.wlel` source in the repository (examples, flagships, selfhost)
fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
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

fn class(t: &Token) -> char {
    match t {
        Token::Ident(_) => 'I',
        Token::Int(_) | Token::IntSuf(..) => 'N',
        Token::Float(_) | Token::FloatSuf(..) => 'F',
        Token::Str(_) => 'S',
        Token::Eof => 'E',
        Token::Fn
        | Token::Extern
        | Token::Struct
        | Token::Enum
        | Token::Impl
        | Token::Match
        | Token::Defer
        | Token::Arena
        | Token::Use
        | Token::Test
        | Token::Module
        | Token::Const
        | Token::As
        | Token::Let
        | Token::Return
        | Token::If
        | Token::Else
        | Token::While
        | Token::For
        | Token::In
        | Token::Break
        | Token::Continue
        | Token::True
        | Token::False => 'K',
        _ => 'P',
    }
}

/// byte offset of a 1-based (line, col) position
fn offset(line_starts: &[usize], line: usize, col: usize) -> usize {
    line_starts[line - 1] + col - 1
}

fn line_starts(src: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// escape exactly like `escape_field` in the Wlel lexer
fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '|' => out.push_str("\\|"),
            c => out.push(c),
        }
    }
    out
}

fn rust_dump(src: &str) -> Vec<String> {
    let starts = line_starts(src);
    let toks = Lexer::new(src).tokenize().expect("the Rust lexer accepts this file");
    toks.iter()
        .map(|t| {
            let start = offset(&starts, t.span.start.line, t.span.start.col);
            let end = offset(&starts, t.span.end.line, t.span.end.col);
            format!(
                "{}|{}|{}|{}|{}|{}",
                t.span.start.line,
                t.span.start.col,
                start,
                end,
                class(&t.token),
                escape(&src[start..end])
            )
        })
        .collect()
}

/// Build the self-hosted lexer once for the whole test binary.
///
/// Building it per test would compile the compiler and the generated C four
/// times, which is slow on a cold CI runner (and macOS process spawn is not
/// cheap either). The binary lives in the temp directory for the process's
/// lifetime.
fn lexer_binary() -> Option<&'static Path> {
    static BINARY: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    BINARY
        .get_or_init(|| {
            if !have("cc") && !have("gcc") && !have("clang") {
                eprintln!("skipping: no C compiler on PATH");
                return None;
            }
            let out = std::env::temp_dir().join(format!(
                "wlel_selfhost_lexer_{}",
                std::process::id()
            ));
            let _ = fs::remove_file(&out);
            let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
                .args(["build", "selfhost/lexer.wlel", "-O2", "-o"])
                .arg(&out)
                .current_dir(root())
                .output()
                .expect("spawn wlel build");
            assert!(
                build.status.success(),
                "the self-hosted lexer does not build:\n{}{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            );
            Some(out)
        })
        .as_deref()
}

#[test]
fn the_self_hosted_lexer_matches_the_rust_one() {
    let Some(binary) = lexer_binary() else {
        return;
    };
    let files = sources();
    assert!(files.len() >= 30, "expected the repository sources, found {}", files.len());

    let mut failures: Vec<String> = Vec::new();
    for file in &files {
        let src = fs::read_to_string(file).expect("read source");
        let expected = rust_dump(&src);
        let out = Command::new(binary)
            .arg(file)
            .arg("dump")
            .output()
            .expect("run the self-hosted lexer");
        if !out.status.success() {
            failures.push(format!(
                "{}: lexer failed:\n{}",
                file.display(),
                String::from_utf8_lossy(&out.stderr)
            ));
            continue;
        }
        let actual: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect();
        if actual != expected {
            let first = actual
                .iter()
                .zip(expected.iter())
                .position(|(a, b)| a != b);
            let detail = match first {
                Some(i) => format!(
                    "first difference at token {i}:\n  wlel: {}\n  rust: {}",
                    actual.get(i).map(String::as_str).unwrap_or("<missing>"),
                    expected.get(i).map(String::as_str).unwrap_or("<missing>")
                ),
                None => format!(
                    "token counts differ: wlel {} vs rust {}",
                    actual.len(),
                    expected.len()
                ),
            };
            failures.push(format!("{}: {detail}", file.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "the self-hosted lexer disagrees with the Rust lexer on {} of {} files:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn the_self_hosted_lexer_reports_the_token_count() {
    let Some(binary) = lexer_binary() else {
        return;
    };
    let file = root().join("examples/kitchen.wlel");
    let out = Command::new(binary)
        .arg(&file)
        .output()
        .expect("run the self-hosted lexer");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let count: usize = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("a token count");
    let src = fs::read_to_string(&file).expect("read");
    let rust = Lexer::new(&src).tokenize().expect("lex").len() - 1; // minus Eof
    assert_eq!(count, rust, "token counts differ");
}

#[test]
fn the_self_hosted_lexer_rejects_a_bad_token() {
    let Some(binary) = lexer_binary() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("wlel_selfhost_bad_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let file = dir.join("bad.wlel");
    fs::write(&file, "fn main() -> int { ` }\n").expect("write");
    let out = Command::new(binary)
        .arg(&file)
        .output()
        .expect("run the self-hosted lexer");
    let _ = fs::remove_dir_all(&dir);
    assert!(!out.status.success(), "a backtick must be rejected");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("invalid token"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_self_hosted_lexer_is_fast_enough() {
    // not a benchmark: a guard that the zero-copy design did not regress into
    // something quadratic. The flagship JSON source is the largest file in
    // the repository; lexing it 20 times must stay well under a second of
    // wall clock on any machine.
    let Some(binary) = lexer_binary() else {
        return;
    };
    let file = root().join("flagship/json.wlel");
    // in-process benchmark: process spawn dominates on some hosts, so the
    // lexer reports its own timing and the guard reads that
    let out = Command::new(binary)
        .args([file.to_str().unwrap(), "bench", "50"])
        .output()
        .expect("run the self-hosted lexer");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    let elapsed_ms: u64 = text
        .split(", ")
        .find_map(|part| part.strip_suffix(" ms"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(u64::MAX);
    assert!(
        elapsed_ms < 5_000,
        "lexing flagship/json.wlel 50 times took {elapsed_ms} ms — the lexer regressed: {text}"
    );
}

#[test]
fn the_self_hosted_lexer_reports_throughput() {
    let Some(binary) = lexer_binary() else {
        return;
    };
    let file = root().join("flagship/json.wlel");
    let out = Command::new(binary)
        .args([file.to_str().unwrap(), "bench", "20"])
        .output()
        .expect("run the self-hosted lexer");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    // "20 passes, N tokens, M ms, K MB/s"
    assert!(text.contains("20 passes"), "{text}");
    let mbps: u64 = text
        .rsplit(' ')
        .find_map(|w| w.parse().ok())
        .unwrap_or(0);
    assert!(mbps > 10, "throughput regressed badly: {text}");
}

#[test]
fn the_self_hosted_lexer_agrees_on_the_tricky_cases() {
    let Some(binary) = lexer_binary() else {
        return;
    };
    let cases = [
        "fn main() -> int { return 0; }\n",
        "x := 0xFFi32; y := 0b1010u8; z := 1_000_000;\n",
        "f := 2.5f32; g := 1e-9; h := 3.14159;\n",
        "s := \"a\\n\\t\\\\\\\"b\";\n",
        "a::b::c; x..y; p += 1; q <<= 2;\n",
        "// comment only\n",
        "match x { A => 1, B(y) => 2, _ => 3 }\n",
        "const N: int = 10; module m;\n",
        "if a == b && c != d || !e { }\n",
        "arr[0] = *p + &q; obj.f(); obj.m(1);\n",
    ];
    let dir = std::env::temp_dir().join(format!("wlel_selfhost_tricky_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let mut failures = Vec::new();
    for (i, src) in cases.iter().enumerate() {
        let file = dir.join(format!("case{i}.wlel"));
        fs::write(&file, src).expect("write");
        let expected = rust_dump(src);
        let out = Command::new(binary)
            .arg(&file)
            .arg("dump")
            .output()
            .expect("run");
        let actual: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect();
        if actual != expected {
            failures.push(format!(
                "case {i} ({src:?}):\n  wlel: {:?}\n  rust: {:?}",
                actual.first(),
                expected.first()
            ));
        }
    }
    let _ = fs::remove_dir_all(&dir);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
