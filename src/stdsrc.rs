//! The Wlel standard library, written in Wlel itself and compiled into the
//! compiler binary. It is spliced into every program that declares
//! `use std;` (see `main::front`), then flows through the normal
//! type-check and monomorphization pipeline — collections are only
//! instantiated for the types a program actually uses.

/// Source marker recorded on every std function/struct so diagnostics can
/// point into the embedded library instead of a user file.
pub const STD_FILE: &str = "<std>";

pub const STD_WL: &str = include_str!("../lib/std.wlel");

/// Lex + parse the embedded std source. Panics on failure: the library is a
/// compiler-internal constant, and a broken std is a build error, not a
/// user-facing diagnostic.
///
/// The result is parsed once per process and then cloned: `use std` is on
/// the path of nearly every compile, and lexing + parsing ~750 lines of
/// library source cost more than cloning the finished AST (measured ~4x).
/// Every definition is tagged with STD_FILE here, so clones are ready to
/// splice.
pub fn parse_std() -> crate::ast::Program {
    static CACHED: std::sync::OnceLock<crate::ast::Program> = std::sync::OnceLock::new();
    CACHED.get_or_init(parse_std_uncached).clone()
}

fn parse_std_uncached() -> crate::ast::Program {
    let toks = crate::lexer::Lexer::new(STD_WL)
        .tokenize()
        .expect("std library lexes");
    let (mut program, errs) = crate::parser::Parser::new(&toks).program();
    assert!(
        errs.is_empty(),
        "std library parses cleanly: {errs:?}"
    );
    for f in program.funcs.iter_mut() {
        f.file = STD_FILE.into();
    }
    for s in program.structs.iter_mut() {
        s.file = STD_FILE.into();
    }
    for e in program.enums.iter_mut() {
        e.file = STD_FILE.into();
    }
    for i in program.impls.iter_mut() {
        i.file = STD_FILE.into();
    }
    program
}

/// Splice the embedded library into a parsed program (structs, enums,
/// impls, functions) — the exact transformation `use std` triggers in the
/// CLI front end. The library definitions come first so user code can
/// shadow nothing it should not, and every one carries STD_FILE so
/// diagnostics and dead-code elimination can tell them apart.
pub fn splice_std(program: &mut crate::ast::Program) {
    let std_prog = parse_std();
    program.structs.splice(0..0, std_prog.structs);
    program.enums.splice(0..0, std_prog.enums);
    program.impls.splice(0..0, std_prog.impls);
    program.funcs.splice(0..0, std_prog.funcs);
}
