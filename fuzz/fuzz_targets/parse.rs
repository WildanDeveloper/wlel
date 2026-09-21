//! Fuzz the Wlel front-end: lexer -> parser -> type checker must never
//! panic, hang, or read out of bounds, no matter the input.
//!
//! Run locally:  cargo fuzz run parse -- -max_total_time=1800
//! (CI: .github/workflows/fuzz.yml, 30 minutes daily)

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // sources are text; lossy-convert so invalid UTF-8 exercises the
    // lexer's error paths as well
    let src = String::from_utf8_lossy(data);
    if src.len() > 64 * 1024 {
        return; // keep individual iterations fast
    }
    let Ok(toks) = wlel::lexer::Lexer::new(&src).tokenize() else {
        return; // lex errors are fine, panics are not
    };
    let (mut prog, _parse_errors) = wlel::parser::Parser::new(&toks).program();
    // checker errors are expected on garbage input; crashes are not
    let _ = wlel::checker::Checker::check(&mut prog);
});
