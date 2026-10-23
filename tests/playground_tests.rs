//! The playground's analysis pipeline (`wlel::playground`), which is what the
//! WebAssembly module exports to the browser.
//!
//! These are native tests: they prove the diagnostics the playground shows are
//! the compiler's own, so the site cannot drift from `wlel check`.

use wlel::playground::{analyze, analyze_json};

const GOOD: &str = "use std;\nfn main() -> int {\n    x := 1;\n    std::println_int(x);\n    return 0;\n}\n";

#[test]
fn a_valid_program_is_ok_and_formats() {
    let a = analyze(GOOD);
    assert!(a.ok, "diagnostics: {:?}", a.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>());
    assert!(a.diagnostics.is_empty());
    let formatted = a.formatted.expect("a parsing program has a canonical form");
    assert!(formatted.contains("fn main() -> int"), "{formatted}");
    // `wlel fmt` canonical form is stable
    assert_eq!(formatted, wlel::fmt::format_source(GOOD).expect("format"));
}

#[test]
fn a_type_error_reports_the_compilers_message_and_span() {
    let src = "fn main() -> int {\n    x := \"s\";\n    return x;\n}\n";
    let a = analyze(src);
    assert!(!a.ok);
    assert_eq!(a.diagnostics.len(), 1, "{:?}", a.diagnostics.len());
    let d = &a.diagnostics[0];
    assert_eq!(d.severity, "error");
    assert_eq!(d.line, 3, "the error is on the `return x` line");
    assert!(d.message.contains("return type mismatch"), "{}", d.message);
    assert!(d.end_line >= d.line);
}

#[test]
fn a_lex_error_stops_before_parsing() {
    let a = analyze("fn main() -> int { x := 0b102; return 0; }");
    assert!(!a.ok);
    assert_eq!(a.diagnostics.len(), 1);
    assert!(a.diagnostics[0].message.contains("base-2"), "{}", a.diagnostics[0].message);
}

#[test]
fn a_syntax_error_is_reported_without_panicking() {
    let a = analyze("fn main() -> int { let }");
    assert!(!a.ok);
    assert!(!a.diagnostics.is_empty());
    assert_eq!(a.diagnostics[0].severity, "error");
}

#[test]
fn file_imports_are_rejected_not_read() {
    let a = analyze("use \"lib/geom.wlel\";\nfn main() -> int { return 0; }");
    assert!(!a.ok);
    assert_eq!(a.diagnostics.len(), 1);
    assert!(
        a.diagnostics[0].message.contains("single buffer"),
        "{}",
        a.diagnostics[0].message
    );
}

#[test]
fn warnings_do_not_make_a_program_fail() {
    let a = analyze("fn main() -> int {\n    unused := 1;\n    return 0;\n}\n");
    assert!(a.ok, "warnings are not errors");
    assert!(a
        .diagnostics
        .iter()
        .any(|d| d.severity == "warning" && d.message.contains("unused variable")));
}

#[test]
fn json_is_well_formed_and_escaped() {
    let src = "use std;\nfn main() -> int {\n    s := \"a\\\"b\";\n    return 0;\n}\n";
    let json = analyze_json(src);
    assert!(json.starts_with("{\"ok\":true,"), "{json}");
    assert!(json.ends_with('}'));
    assert!(json.contains("\"formatted\":\""));

    // the formatted source contains quotes and newlines: both must be escaped,
    // so the document itself must contain no literal newline
    let formatted_start = json.find("\"formatted\":").expect("formatted key");
    let tail = &json[formatted_start..];
    assert!(!tail.contains('\n'), "a raw newline leaked into the JSON string");
    assert!(tail.contains("\\n"), "newlines should be \\n escapes");
    assert!(tail.contains("\\\""), "quotes should be \\\" escapes");

    // balanced quotes outside escapes: a cheap validity check
    let mut escaped = false;
    let mut quotes = 0;
    for c in json.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '"' => quotes += 1,
            _ => {}
        }
    }
    assert_eq!(quotes % 2, 0, "unbalanced quotes in {json}");
}

#[test]
fn json_for_an_error_carries_position_and_severity() {
    let json = analyze_json("fn main() -> int { let }");
    assert!(json.starts_with("{\"ok\":false,"), "{json}");
    assert!(json.contains("\"severity\":\"error\""), "{json}");
    assert!(json.contains("\"line\":1"), "{json}");
    assert!(json.contains("\"formatted\":null"), "{json}");
}
