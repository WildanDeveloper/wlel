use crate::ast::*;
use crate::token::Token;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub msg: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for ParseError {}

type PResult<T> = Result<T, ParseError>;

fn err<T>(msg: impl Into<String>) -> PResult<T> {
    Err(ParseError { msg: msg.into() })
}

pub struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(toks: &'a [Token]) -> Self {
        Self { toks, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.toks[self.pos]
    }

    fn peek_at(&self, off: usize) -> &Token {
        &self.toks[(self.pos + off).min(self.toks.len() - 1)]
    }

    fn advance(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Token) -> bool {
        if *self.peek() == *t {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Token) -> PResult<()> {
        if *self.peek() == *t {
            self.advance();
            Ok(())
        } else {
            err(format!("expected {:?}, found {:?}", t, self.peek()))
        }
    }

    pub fn program(mut self) -> PResult<Program> {
        let mut funcs = Vec::new();
        while *self.peek() != Token::Eof {
            funcs.push(self.func_def()?);
        }
        Ok(Program { funcs })
    }

    fn func_def(&mut self) -> PResult<FuncDef> {
        self.expect(&Token::Fn)?;
        let name = self.ident()?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if *self.peek() != Token::RParen {
            loop {
                let pname = self.ident()?;
                let ty = if self.eat(&Token::Colon) {
                    Some(self.type_name()?)
                } else {
                    None
                };
                params.push(Param { name: pname, ty });
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
        }
        self.expect(&Token::RParen)?;
        let ret_type = if self.eat(&Token::Arrow) {
            Some(self.type_name()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(FuncDef { name, params, ret_type, body })
    }

    fn type_name(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            t => err(format!("expected type name, found {:?}", t)),
        }
    }

    fn ident(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            t => err(format!("expected identifier, found {:?}", t)),
        }
    }

    fn block(&mut self) -> PResult<Block> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while *self.peek() != Token::RBrace && *self.peek() != Token::Eof {
            stmts.push(self.stmt()?);
        }
        self.expect(&Token::RBrace)?;
        Ok(Block(stmts))
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        match self.peek().clone() {
            Token::Let => {
                self.advance();
                let name = self.ident()?;
                // `let x := expr` (inferred) or `let x: T = expr` (annotated)
                let ty = if self.eat(&Token::Colon) {
                    Some(self.type_name()?)
                } else {
                    None
                };
                if ty.is_some() {
                    self.expect(&Token::Assign)?;
                } else {
                    self.expect(&Token::Define)?;
                }
                let e = self.expr(0)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Let(name, ty, e))
            }
            // bare `x := expr;` — inference-first declaration (canonical form)
            _ if matches!(self.peek(), Token::Ident(_))
                && matches!(self.peek_at(1), Token::Define) =>
            {
                let name = self.ident()?;
                self.advance(); // :=
                let e = self.expr(0)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Let(name, None, e))
            }
            Token::If => Ok(Stmt::If(self.if_stmt()?)),
            Token::While => {
                self.advance();
                let cond = self.expr(0)?;
                let body = self.block()?;
                Ok(Stmt::While(cond, body))
            }
            Token::Return => {
                self.advance();
                let e = if *self.peek() == Token::Semicolon {
                    None
                } else {
                    Some(self.expr(0)?)
                };
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::Return(e))
            }
            _ => {
                if let Token::Ident(_) = *self.peek() {
                    let is_assign = matches!(self.peek_at(1), Token::Assign);
                    let is_call = matches!(self.peek_at(1), Token::LParen);
                    if is_assign {
                        let name = self.ident()?;
                        self.expect(&Token::Assign)?;
                        let e = self.expr(0)?;
                        self.expect(&Token::Semicolon)?;
                        return Ok(Stmt::Assign(name, e));
                    }
                    if is_call {
                        let e = self.expr(0)?;
                        self.expect(&Token::Semicolon)?;
                        return Ok(Stmt::ExprStmt(e));
                    }
                }
                let e = self.expr(0)?;
                self.expect(&Token::Semicolon)?;
                Ok(Stmt::ExprStmt(e))
            }
        }
    }

    fn if_stmt(&mut self) -> PResult<IfStmt> {
        self.expect(&Token::If)?;
        let cond = self.expr(0)?;
        let then_body = self.block()?;
        let else_branch = if self.eat(&Token::Else) {
            if *self.peek() == Token::If {
                Some(ElseBranch::If(Box::new(self.if_stmt()?)))
            } else {
                Some(ElseBranch::Block(self.block()?))
            }
        } else {
            None
        };
        Ok(IfStmt { cond, then_body, else_branch })
    }

    fn lbp(t: &Token) -> Option<u8> {
        match t {
            Token::OrOr => Some(1),
            Token::AndAnd => Some(2),
            Token::Eq | Token::NotEq | Token::Lt | Token::Gt | Token::LtEq | Token::GtEq => Some(3),
            Token::Plus | Token::Minus => Some(4),
            Token::Star | Token::Slash | Token::Percent => Some(5),
            _ => None,
        }
    }

    fn binop(t: &Token) -> BinOp {
        match t {
            Token::OrOr => BinOp::Or,
            Token::AndAnd => BinOp::And,
            Token::Eq => BinOp::Eq,
            Token::NotEq => BinOp::Ne,
            Token::Lt => BinOp::Lt,
            Token::Gt => BinOp::Gt,
            Token::LtEq => BinOp::Le,
            Token::GtEq => BinOp::Ge,
            Token::Plus => BinOp::Add,
            Token::Minus => BinOp::Sub,
            Token::Star => BinOp::Mul,
            Token::Slash => BinOp::Div,
            Token::Percent => BinOp::Mod,
            other => unreachable!("binop on non-operator {:?}", other),
        }
    }

    fn expr(&mut self, min_bp: u8) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        loop {
            let bp = match Self::lbp(self.peek()) {
                Some(b) => b,
                None => break,
            };
            if bp < min_bp {
                break;
            }
            let op = Self::binop(self.peek());
            self.advance();
            let rhs = self.expr(bp + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.eat(&Token::Minus) {
            return Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?)));
        }
        if self.eat(&Token::Bang) {
            return Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.peek().clone();
        match t {
            Token::Int(v) => {
                self.advance();
                Ok(Expr::Int(v))
            }
            Token::Float(v) => {
                self.advance();
                Ok(Expr::Float(v))
            }
            Token::Str(s) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Token::Ident(name) => {
                self.advance();
                if self.eat(&Token::LParen) {
                    let mut args = Vec::new();
                    if *self.peek() != Token::RParen {
                        loop {
                            args.push(self.expr(0)?);
                            if !self.eat(&Token::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            Token::LParen => {
                self.advance();
                let e = self.expr(0)?;
                self.expect(&Token::RParen)?;
                Ok(e)
            }
            other => err(format!("unexpected token {:?}", other)),
        }
    }
}
