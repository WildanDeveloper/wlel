use crate::token::Token;

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

pub struct Lexer {
    src: Vec<u8>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Self {
            src: src.as_bytes().to_vec(),
            pos: 0,
            line: 1,
            col: 1,
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
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => break,
            }
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        let mut out = Vec::new();
        loop {
            self.skip_trivia();
            // remember where the token starts so errors point at it
            let (start_line, start_col) = (self.line, self.col);
            match self.bump() {
                None => {
                    out.push(Token::Eof);
                    return Ok(out);
                }
                Some(c) => {
                    match self.token(c) {
                        Ok(t) => out.push(t),
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
            b'.' => Ok(Token::Dot),
            b';' => Ok(Token::Semicolon),
            b':' => {
                if self.eat(b'=') {
                    Ok(Token::Define)
                } else {
                    Ok(Token::Colon)
                }
            }
            b'+' => Ok(Token::Plus),
            b'*' => Ok(Token::Star),
            b'%' => Ok(Token::Percent),
            b'-' => {
                if self.eat(b'>') {
                    Ok(Token::Arrow)
                } else {
                    Ok(Token::Minus)
                }
            }
            b'/' => Ok(Token::Slash),
            b'=' => {
                if self.eat(b'=') {
                    Ok(Token::Eq)
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
            b'<' => {
                if self.eat(b'=') {
                    Ok(Token::LtEq)
                } else {
                    Ok(Token::Lt)
                }
            }
            b'>' => {
                if self.eat(b'=') {
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
            b'|' => {
                if self.eat(b'|') {
                    Ok(Token::OrOr)
                } else {
                    self.err("unexpected '|' (did you mean '||'?)")
                }
            }
            other => self.err(format!("unexpected character '{}'", other as char)),
        }
    }

    fn number(&mut self, first: u8) -> Result<Token, LexError> {
        let mut s = String::new();
        s.push(first as char);
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c as char);
                self.bump();
            } else if c == b'.' && !is_float && self.peek2().map_or(false, |d| d.is_ascii_digit()) {
                is_float = true;
                s.push('.');
                self.bump();
            } else {
                break;
            }
        }
        if is_float {
            match s.parse::<f64>() {
                Ok(v) => Ok(Token::Float(v)),
                Err(_) => self.err(format!("bad float literal '{s}'")),
            }
        } else {
            match s.parse::<i64>() {
                Ok(v) => Ok(Token::Int(v)),
                Err(_) => self.err(format!("integer literal '{s}' out of range")),
            }
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
            "struct" => Token::Struct,
            "let" => Token::Let,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "while" => Token::While,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<Token> {
        Lexer::new(src).tokenize().expect("should lex")
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
    fn utf8_strings() {
        let toks = lex("\"apa kabar 🇮🇩\"");
        assert_eq!(toks[0], Token::Str("apa kabar 🇮🇩".into()));
    }
}
