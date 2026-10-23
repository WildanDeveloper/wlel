//! Front-end analysis for the WASM playground.
//!
//! This is the same pipeline `wlel check` runs — lex, parse with recovery,
//! splice the embedded std when `use std;` is present, type-check — minus the
//! filesystem. A buffer that imports another file is rejected with a clear
//! message instead of trying to read it.
//!
//! The result is a JSON document so the browser needs no serialization
//! library on either side:
//!
//! ```json
//! {"ok":false,
//!  "diagnostics":[{"line":3,"col":12,"endLine":3,"endCol":17,
//!                  "severity":"error","message":"..."}],
//!  "formatted":null}
//! ```
//!
//! `formatted` is the canonical `wlel fmt` output when the source parses, so
//! the playground can offer a "format" button without a second code path.

use crate::ast::Program;
use crate::checker::Checker;
use crate::fmt;
use crate::lexer::Lexer;
use crate::parser::Parser;

/// One diagnostic, in source terms (1-based line and column).
pub struct Diagnostic {
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub severity: &'static str,
    pub message: String,
}

pub struct Analysis {
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub formatted: Option<String>,
}

/// Run the front-end over a single buffer.
pub fn analyze(src: &str) -> Analysis {
    let mut diagnostics = Vec::new();

    let tokens = match Lexer::new(src).tokenize() {
        Ok(tokens) => tokens,
        Err(e) => {
            diagnostics.push(Diagnostic {
                line: e.line,
                col: e.col,
                end_line: e.line,
                end_col: e.col,
                severity: "error",
                message: e.msg,
            });
            return Analysis { ok: false, diagnostics, formatted: None };
        }
    };

    let (mut program, errors) = Parser::new(&tokens).program();
    for e in &errors {
        diagnostics.push(Diagnostic {
            line: e.span.start.line,
            col: e.span.start.col,
            end_line: e.span.end.line,
            end_col: e.span.end.col,
            severity: "error",
            message: e.msg.clone(),
        });
    }
    if !errors.is_empty() {
        return Analysis { ok: false, diagnostics, formatted: None };
    }

    // the playground has no filesystem: a file import is an error, not a panic
    if let Some(import) = program.uses.iter().find(|u| u.path.is_some()) {
        diagnostics.push(Diagnostic {
            line: import.span.start.line,
            col: import.span.start.col,
            end_line: import.span.end.line,
            end_col: import.span.end.col,
            severity: "error",
            message: "the playground is a single buffer: file imports are not available"
                .to_string(),
        });
        return Analysis { ok: false, diagnostics, formatted: None };
    }

    if program.uses.iter().any(|u| u.path.is_none()) {
        crate::stdsrc::splice_std(&mut program);
    }

    let formatted = fmt::format_source(src).ok();
    let warnings = match Checker::check(&mut program) {
        Ok(warnings) => warnings,
        Err(e) => {
            diagnostics.push(Diagnostic {
                line: e.span.start.line,
                col: e.span.start.col,
                end_line: e.span.end.line,
                end_col: e.span.end.col,
                severity: "error",
                message: e.msg,
            });
            return Analysis { ok: false, diagnostics, formatted };
        }
    };
    for w in warnings {
        diagnostics.push(Diagnostic {
            line: w.span.start.line,
            col: w.span.start.col,
            end_line: w.span.end.line,
            end_col: w.span.end.col,
            severity: "warning",
            message: w.msg,
        });
    }

    Analysis { ok: true, diagnostics, formatted }
}

/// `analyze` as the JSON the playground consumes.
pub fn analyze_json(src: &str) -> String {
    let analysis = analyze(src);
    let mut out = String::with_capacity(256);
    out.push_str("{\"ok\":");
    out.push_str(if analysis.ok { "true" } else { "false" });
    out.push_str(",\"diagnostics\":[");
    for (i, d) in analysis.diagnostics.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"line\":");
        out.push_str(&d.line.to_string());
        out.push_str(",\"col\":");
        out.push_str(&d.col.to_string());
        out.push_str(",\"endLine\":");
        out.push_str(&d.end_line.to_string());
        out.push_str(",\"endCol\":");
        out.push_str(&d.end_col.to_string());
        out.push_str(",\"severity\":\"");
        out.push_str(d.severity);
        out.push_str("\",\"message\":\"");
        escape_into(&mut out, &d.message);
        out.push_str("\"}");
    }
    out.push_str("],\"formatted\":");
    match &analysis.formatted {
        Some(text) => {
            out.push('"');
            escape_into(&mut out, text);
            out.push('"');
        }
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn escape_into(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}

/// The parser needs to see the same `Program` the loader would build, but
/// without touching the disk. Exposed for tests so the two stay in sync.
pub fn program_from(src: &str) -> Result<Program, Vec<String>> {
    let tokens = Lexer::new(src)
        .tokenize()
        .map_err(|e| vec![e.to_string()])?;
    let (mut program, errors) = Parser::new(&tokens).program();
    if !errors.is_empty() {
        return Err(errors.iter().map(|e| e.to_string()).collect());
    }
    if program.uses.iter().any(|u| u.path.is_none()) {
        crate::stdsrc::splice_std(&mut program);
    }
    Ok(program)
}
