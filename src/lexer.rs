use crate::span::Span;
use crate::token::{SpannedToken, Token};

#[derive(Debug, Clone)]
pub struct LexError {
    pub line: usize,
    pub col: usize,
    pub msg: String,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for LexError {}

/// a `// line comment` seen between tokens (block comments do not exist in
/// Wlel); `text` is everything after the `//` on that line
#[derive(Debug, Clone)]
pub struct Comment {
    pub line: usize,
    pub col: usize,
    pub text: String,
}

pub struct Lexer {
    src: Vec<u8>,
    pos: usize,
    line: usize,
    col: usize,
    comments: Vec<Comment>,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Self {
            src: src.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            col: 1,
            comments: Vec::new(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.src.get(self.pos + 1).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, LexError> {
        Err(LexError {
            line: self.line,
            col: self.col,
            msg: msg.into(),
        })
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' => {
                    self.bump();
                }
                Some(b'/') if self.peek2() == Some(b'/') => {
                    // record the comment (the formatter replays it) and skip it
                    let (cline, ccol) = (self.line, self.col);
                    let start = self.pos + 2; // past the two slashes
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                    let text = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
                    self.comments.push(Comment { line: cline, col: ccol, text });
                }
                _ => break,
            }
        }
    }

    pub fn tokenize(self) -> Result<Vec<SpannedToken>, LexError> {
        self.tokenize_with_comments().map(|(t, _)| t)
    }

    /// tokens plus every comment seen, in source order (used by `wlel fmt`)
    pub fn tokenize_with_comments(mut self) -> Result<(Vec<SpannedToken>, Vec<Comment>), LexError> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia();
            // remember where the token starts so errors point at it
            let (start_line, start_col) = (self.line, self.col);
            match self.bump() {
                None => {
                    out.push(SpannedToken::new(
                        Token::Eof,
                        Span::point(start_line, start_col),
                    ));
                    return Ok((out, self.comments));
                }
                Some(c) => {
                    match self.token(c) {
                        Ok(t) => out.push(SpannedToken::new(
                            t,
                            Span::new(start_line, start_col, self.line, self.col),
                        )),
                        Err(mut e) => {
                            // errors raised right after the bump refer to the
                            // token start we snapshotted
                            if e.line == self.line && e.col == self.col {
                                e.line = start_line;
                                e.col = start_col;
                            }
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    fn token(&mut self, c: u8) -> Result<Token, LexError> {
        match c {
            b'0'..=b'9' => self.number(c),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => Ok(self.ident(c)),
            b'"' => self.string(),
            b'(' => Ok(Token::LParen),
            b')' => Ok(Token::RParen),
            b'{' => Ok(Token::LBrace),
            b'}' => Ok(Token::RBrace),
            b',' => Ok(Token::Comma),
            b'.' => {
                if self.eat(b'.') {
                    Ok(Token::DotDot)
                } else {
                    Ok(Token::Dot)
                }
            }
            b'[' => Ok(Token::LBracket),
            b']' => Ok(Token::RBracket),
            b':' => {
                if self.eat(b':') {
                    Ok(Token::DoubleColon)
                } else if self.eat(b'=') {
                    Ok(Token::Define)
                } else {
                    Ok(Token::Colon)
                }
            }
            b';' => Ok(Token::Semicolon),
            b'+' => {
                if self.eat(b'=') {
                    Ok(Token::PlusEq)
                } else {
                    Ok(Token::Plus)
                }
            }
            b'*' => {
                if self.eat(b'=') {
                    Ok(Token::StarEq)
                } else {
                    Ok(Token::Star)
                }
            }
            b'%' => {
                if self.eat(b'=') {
                    Ok(Token::PercentEq)
                } else {
                    Ok(Token::Percent)
                }
            }
            b'-' => {
                if self.eat(b'>') {
                    Ok(Token::Arrow)
                } else if self.eat(b'=') {
                    Ok(Token::MinusEq)
                } else {
                    Ok(Token::Minus)
                }
            }
            b'/' => {
                if self.eat(b'=') {
                    Ok(Token::SlashEq)
                } else {
                    Ok(Token::Slash)
                }
            }
            b'=' => {
                if self.eat(b'=') {
                    Ok(Token::Eq)
                } else if self.eat(b'>') {
                    Ok(Token::FatArrow)
                } else {
                    Ok(Token::Assign)
                }
            }
            b'!' => {
                if self.eat(b'=') {
                    Ok(Token::NotEq)
                } else {
                    Ok(Token::Bang)
                }
            }
            b'|' => {
                if self.eat(b'|') {
                    Ok(Token::OrOr)
                } else {
                    Ok(Token::BitOr)
                }
            }
            b'^' => Ok(Token::BitXor),
            b'~' => Ok(Token::BitNot),
            b'<' => {
                if self.eat(b'<') {
                    Ok(Token::Shl)
                } else if self.eat(b'=') {
                    Ok(Token::LtEq)
                } else {
                    Ok(Token::Lt)
                }
            }
            b'>' => {
                if self.eat(b'>') {
                    Ok(Token::Shr)
                } else if self.eat(b'=') {
                    Ok(Token::GtEq)
                } else {
                    Ok(Token::Gt)
                }
            }
            b'&' => {
                if self.eat(b'&') {
                    Ok(Token::AndAnd)
                } else {
                    Ok(Token::Amp)
                }
            }
            b'?' => Ok(Token::Question),
            other => self.err(format!("unexpected character '{}'", other as char)),
        }
    }

    fn number(&mut self, first: u8) -> Result<Token, LexError> {
        // radix prefixes: 0x hex, 0b binary, 0o octal
        if first == b'0' {
            if let Some(c) = self.peek() {
                let radix = match c {
                    b'x' | b'X' => Some(16),
                    b'b' | b'B' => Some(2),
                    b'o' | b'O' => Some(8),
                    _ => None,
                };
                if let Some(radix) = radix {
                    self.bump();
                    let mut digits = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_alphanumeric() || c == b'_' {
                            digits.push(c as char);
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    let cleaned_all: String =
                        digits.chars().filter(|c| *c != '_').collect();
                    if cleaned_all.is_empty() {
                        return self.err(format!(
                            "malformed number: expected digits after '0{}'",
                            radix_char(radix)
                        ));
                    }
                    // hex digits overlap suffix letters; strip the widest
                    // known suffix from the tail
                    let (cleaned, suffix) = split_radix_suffix(&cleaned_all);
                    if cleaned.is_empty() {
                        return self.err(format!(
                            "malformed number: expected digits after '0{}'",
                            radix_char(radix)
                        ));
                    }
                    let v = match i128::from_str_radix(cleaned, radix) {
                        Ok(v) => v,
                        Err(_) => {
                            return self
                                .err(format!("integer literal '0{}{digits}' out of range", radix_char(radix)))
                        }
                    };
                    return match suffix {
                        Some(suf) => self.suffixed_int(v, &suf),
                        None => self.plain_int(v),
                    };
                }
            }
        }
        let mut s = String::new();
        s.push(first as char);
        let mut is_float = false;
        let mut has_exp = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'_' {
                s.push(c as char);
                self.bump();
            } else if c == b'.' && !is_float && !has_exp && self.peek2().is_some_and(|d| d.is_ascii_digit()) {
                is_float = true;
                s.push('.');
                self.bump();
            } else if (c == b'e' || c == b'E') && !has_exp {
                // lookahead to confirm scientific notation
                let next = self.peek2();
                let next_is_sign = matches!(next, Some(b'+' | b'-'));
                let next_is_digit = next.is_some_and(|d| d.is_ascii_digit());
                let after_sign_is_digit = self.src.get(self.pos + 2).copied().is_some_and(|d| d.is_ascii_digit());
                if next_is_digit || (next_is_sign && after_sign_is_digit) {
                    is_float = true;
                    has_exp = true;
                    s.push(c as char);
                    self.bump();
                    if self.peek().is_some_and(|d| d == b'+' || d == b'-') {
                        s.push(self.bump().unwrap() as char);
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        let cleaned: String = s.chars().filter(|c| *c != '_').collect();
        let suffix = self.literal_suffix();
        if is_float {
            let v = match cleaned.parse::<f64>() {
                Ok(v) => v,
                Err(_) => return self.err(format!("bad float literal '{s}'")),
            };
            match suffix {
                Some(suf) => match suf.as_str() {
                    "f32" | "f64" => Ok(Token::FloatSuf(v, suf)),
                    other => self.err(format!("unknown literal suffix '{other}' on float")),
                },
                None => Ok(Token::Float(v)),
            }
        } else {
            let v = match cleaned.parse::<i128>() {
                Ok(v) => v,
                Err(_) => return self.err(format!("integer literal '{s}' out of range")),
            };
            match suffix {
                Some(suf) => self.suffixed_int(v, &suf),
                None => self.plain_int(v),
            }
        }
    }

    /// width suffix directly attached to a numeric literal (`10u8`, `2.5f32`)
    fn literal_suffix(&mut self) -> Option<String> {
        if !self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                s.push(c as char);
                self.bump();
            } else {
                break;
            }
        }
        Some(s)
    }

    /// unsuffixed literals are i64-valued
    fn plain_int(&mut self, v: i128) -> Result<Token, LexError> {
        if v > i64::MAX as i128 || v < i64::MIN as i128 {
            return self.err(format!("integer literal '{v}' out of range (i64)"));
        }
        Ok(Token::Int(v as i64))
    }

    /// validate an integer-literal suffix against the known width names;
    /// suffixed literals may span the full u64 range
    fn suffixed_int(&mut self, v: i128, suf: &str) -> Result<Token, LexError> {
        match suf {
            "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize"
            | "byte" | "char" => {
                if v > u64::MAX as i128 || v < i64::MIN as i128 {
                    return self.err(format!("integer literal '{v}{suf}' out of range"));
                }
                Ok(Token::IntSuf(v as u64, suf.into()))
            }
            "f32" | "f64" => Ok(Token::FloatSuf(v as f64, suf.into())),
            other => self.err(format!("unknown literal suffix '{other}'")),
        }
    }

    fn ident(&mut self, first: u8) -> Token {
        let mut s = String::new();
        s.push(first as char);
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                s.push(c as char);
                self.bump();
            } else {
                break;
            }
        }
        match s.as_str() {
            "fn" => Token::Fn,
            "extern" => Token::Extern,
            "struct" => Token::Struct,
            "enum" => Token::Enum,
            "impl" => Token::Impl,
            "match" => Token::Match,
            "use" => Token::Use,
            "test" => Token::Test,
            "as" => Token::As,
            "defer" => Token::Defer,
            "arena" => Token::Arena,
            "let" => Token::Let,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
            "for" => Token::For,
            "in" => Token::In,
            "break" => Token::Break,
            "continue" => Token::Continue,
            "true" => Token::True,
            "false" => Token::False,
            _ => Token::Ident(s),
        }
    }

    fn string(&mut self) -> Result<Token, LexError> {
        let mut s = String::new();
        loop {
            match self.bump() {
                None => return self.err("unterminated string literal"),
                Some(b'"') => break,
                Some(b'\\') => match self.bump() {
                    Some(b'n') => s.push('\n'),
                    Some(b't') => s.push('\t'),
                    Some(b'r') => s.push('\r'),
                    Some(b'"') => s.push('"'),
                    Some(b'\\') => s.push('\\'),
                    Some(b'0') => s.push('\0'),
                    Some(other) => {
                        return self.err(format!("invalid escape '\\{}'", other as char));
                    }
                    None => return self.err("unterminated string literal"),
                },
                Some(c) if c < 0x80 => s.push(c as char),
                Some(c) => {
                    // multi-byte UTF-8: collect continuation bytes
                    let len = utf8_len(c);
                    let mut buf = vec![c];
                    for _ in 1..len {
                        match self.bump() {
                            Some(b) => buf.push(b),
                            None => return self.err("unterminated string literal"),
                        }
                    }
                    match std::str::from_utf8(&buf) {
                        Ok(st) => s.push_str(st),
                        Err(_) => return self.err("invalid UTF-8 in string literal"),
                    }
                }
            }
        }
        Ok(Token::Str(s))
    }
}

fn utf8_len(first: u8) -> usize {
    if first >= 0xF0 {
        4
    } else if first >= 0xE0 {
        3
    } else {
        2
    }
}

/// width suffixes usable on radix (hex/bin/oct) literals: only ones starting
/// with a non-hex-digit letter, so `0xABu8` is unambiguous but `0x8F32` stays
/// a plain hex number rather than `0x8` + `f32`
const RADIX_SUFFIXES: [&str; 9] = ["usize", "i16", "i32", "i64", "u16", "u32", "u64", "i8", "u8"];

/// strip a width suffix from the end of radix digits ("FFi32" -> "FF", "i32")
fn split_radix_suffix(cleaned: &str) -> (&str, Option<String>) {
    for suf in RADIX_SUFFIXES {
        if let Some(head) = cleaned.strip_suffix(suf) {
            return (head, Some(suf.to_string()));
        }
    }
    (cleaned, None)
}

fn radix_char(r: u32) -> char {
    match r {
        16 => 'x',
        2 => 'b',
        _ => 'o',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<Token> {
        Lexer::new(src)
            .tokenize()
            .expect("should lex")
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    #[test]
    fn token_spans_tracked() {
        let toks = Lexer::new("let x := 42;").tokenize().expect("lex");
        assert_eq!(toks[0].span.start.line, 1);
        assert_eq!(toks[0].span.start.col, 1);
        assert_eq!(toks[1].span.start.line, 1);
        assert_eq!(toks[1].span.start.col, 5); // 'x' starts at col 5
    }

    #[test]
    fn basic_tokens() {
        let toks = lex("let x := 42;");
        assert_eq!(
            toks,
            vec![
                Token::Let,
                Token::Ident("x".into()),
                Token::Define,
                Token::Int(42),
                Token::Semicolon,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn float_and_ops() {
        let toks = lex("1.5 + 2 * 3 <= 4 != 5 && true || false -> int");
        assert_eq!(
            toks,
            vec![
                Token::Float(1.5),
                Token::Plus,
                Token::Int(2),
                Token::Star,
                Token::Int(3),
                Token::LtEq,
                Token::Int(4),
                Token::NotEq,
                Token::Int(5),
                Token::AndAnd,
                Token::True,
                Token::OrOr,
                Token::False,
                Token::Arrow,
                Token::Ident("int".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn strings_with_escapes() {
        // wlel source: "hi\nthere \"x\""
        let src = "\"hi\\nthere \\\"x\\\"\"";
        let toks = lex(src);
        assert_eq!(toks[0], Token::Str("hi\nthere \"x\"".into()));
    }

    #[test]
    fn comments_are_skipped() {
        let toks = lex("// hello\n1 // trailing\n2");
        assert_eq!(toks, vec![Token::Int(1), Token::Int(2), Token::Eof]);
    }

    #[test]
    fn error_position() {
        let err = Lexer::new("let ok = 1;\n@").tokenize().unwrap_err();
        assert_eq!((err.line, err.col), (2, 1));
    }

    #[test]
    fn token_spans_across_lines() {
        let toks = Lexer::new("let ok = 1;\n  foo").tokenize().expect("lex");
        assert_eq!(toks[4].span.start.line, 1);
        let last = &toks[5];
        assert_eq!((last.span.start.line, last.span.start.col), (2, 3));
    }

    #[test]
    fn literal_width_suffixes() {
        let toks = Lexer::new("10u8 0xFFi32 2.5f32 5usize 1i64 7u64 3f64 2byte 1char")
            .tokenize()
            .expect("lex");
        assert_eq!(toks[0], Token::IntSuf(10, "u8".into()));
        assert_eq!(toks[1], Token::IntSuf(255, "i32".into()));
        assert_eq!(toks[2], Token::FloatSuf(2.5, "f32".into()));
        assert_eq!(toks[3], Token::IntSuf(5, "usize".into()));
        assert_eq!(toks[4], Token::IntSuf(1, "i64".into()));
        assert_eq!(toks[5], Token::IntSuf(7, "u64".into()));
        assert_eq!(toks[6], Token::FloatSuf(3.0, "f64".into()));
        assert_eq!(toks[7], Token::IntSuf(2, "byte".into()));
        assert_eq!(toks[8], Token::IntSuf(1, "char".into()));
    }

    #[test]
    fn u64_max_literal_works() {
        let toks = Lexer::new("18446744073709551615u64 0xFFFFFFFFFFFFFFFFu64")
            .tokenize()
            .expect("lex");
        assert_eq!(toks[0], Token::IntSuf(u64::MAX, "u64".into()));
        assert_eq!(toks[1], Token::IntSuf(u64::MAX, "u64".into()));
    }

    #[test]
    fn unknown_literal_suffix_rejected() {
        let err = Lexer::new("10quux").tokenize().unwrap_err();
        assert!(err.msg.contains("unknown literal suffix"), "{err}");
    }

    #[test]
    fn ident_after_number_still_works() {
        // letters directly after digits are treated as suffix attempts,
        // but a space-separated identifier is untouched
        let toks = Lexer::new("10 x").tokenize().expect("lex");
        assert_eq!(toks[0], Token::Int(10));
        assert_eq!(toks[1], Token::Ident("x".into()));
    }

    #[test]
    fn utf8_strings() {
        let toks = lex("\"hello there 🇮🇩\"");
        assert_eq!(toks[0], Token::Str("hello there 🇮🇩".into()));
    }
}
