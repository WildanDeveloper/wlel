#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),

    // keywords
    Fn,
    Struct,
    Defer,
    Use,
    As,
    Let,
    Return,
    If,
    Else,
    While,
    True,
    False,

    // punctuation / operators
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Dot,
    LBracket,
    RBracket,
    DoubleColon,
    Semicolon,
    Colon,
    Assign,  // =
    Define,  // :=
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Eq,      // ==
    NotEq,   // !=
    Lt,
    Gt,
    LtEq,
    GtEq,
    Amp,     // &
    AndAnd,
    OrOr,
    Arrow,   // ->
    Eof,
}
