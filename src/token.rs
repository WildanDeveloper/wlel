use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Ident(String),
    Int(i64),
    /// integer literal with a width suffix: `10u8`, `0xFFi32`, `5usize`.
    /// value spans the full u64 range (`18446744073709551615u64`)
    IntSuf(u64, String),
    Float(f64),
    /// float literal with a width suffix: `2.5f32`, `1.0f64`
    FloatSuf(f64, String),
    Str(String),

    // keywords
    Fn,
    Struct,
    Defer,
    Arena,
    Use,
    As,
    Let,
    Return,
    If,
    Else,
    While,
    For,
    In,
    Break,
    Continue,
    True,
    False,

    // punctuation / operators
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Dot,
    DotDot, // ..
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
    PlusEq,   // +=
    MinusEq,  // -=
    StarEq,   // *=
    SlashEq,  // /=
    PercentEq, // %=
    BitOr,   // |
    BitXor,  // ^
    BitNot,  // ~
    Shl,     // <<
    Shr,     // >>
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

#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

impl SpannedToken {
    pub fn new(token: Token, span: Span) -> Self {
        Self { token, span }
    }
}

impl PartialEq for SpannedToken {
    fn eq(&self, other: &Self) -> bool {
        self.token == other.token
    }
}

impl PartialEq<Token> for SpannedToken {
    fn eq(&self, other: &Token) -> bool {
        &self.token == other
    }
}

impl PartialEq<SpannedToken> for Token {
    fn eq(&self, other: &SpannedToken) -> bool {
        self == &other.token
    }
}

impl std::ops::Deref for SpannedToken {
    type Target = Token;
    fn deref(&self) -> &Self::Target {
        &self.token
    }
}

