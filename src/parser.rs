use crate::ast::*;
use crate::span::{Pos, Span, Spanned};
use crate::token::{SpannedToken, Token};

#[derive(Debug, Clone)]
pub struct ParseError {
    pub msg: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.span, self.msg)
    }
}

impl std::error::Error for ParseError {}

type PResult<T> = Result<T, ParseError>;

fn err_at<T>(span: Span, msg: impl Into<String>) -> PResult<T> {
    Err(ParseError { msg: msg.into(), span })
}

pub struct Parser<'a> {
    toks: &'a [SpannedToken],
    pos: usize,
    /// end position of the last consumed token, for node spans
    last_end: Pos,
    /// all syntax errors found so far (recovery keeps parsing after each)
    errors: Vec<ParseError>,
}

impl<'a> Parser<'a> {
    pub fn new(toks: &'a [SpannedToken]) -> Self {
        Self {
            toks,
            pos: 0,
            last_end: Pos { line: 1, col: 1 },
            errors: Vec::new(),
        }
    }

    fn peek(&self) -> &Token {
        &self.toks[self.pos].token
    }

    fn peek_span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn peek_at(&self, off: usize) -> &Token {
        &self.toks[(self.pos + off).min(self.toks.len() - 1)].token
    }

    fn advance(&mut self) -> Token {
        let t = self.toks[self.pos].token.clone();
        self.last_end = self.toks[self.pos].span.end;
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
            err_at(
                self.peek_span(),
                format!("expected {:?}, found {:?}", t, self.peek()),
            )
        }
    }

    /// span from `start` to the end of the last consumed token
    fn span_from(&self, start: Span) -> Span {
        start.to(Span::point(self.last_end.line, self.last_end.col))
    }

    fn node<T>(&self, start: Span, node: T) -> Spanned<T> {
        Spanned::new(node, self.span_from(start))
    }

    /// Parse a whole program. Recovery keeps parsing past each syntax error,
    /// so one compile reports every syntax problem, not just the first.
    /// Returns the (partial) program plus all errors found.
    pub fn program(mut self) -> (Program, Vec<ParseError>) {
        let mut structs = Vec::new();
        let mut funcs = Vec::new();
        let mut uses = Vec::new();
        while *self.peek() != Token::Eof {
            let before = self.pos;
            let result = match self.peek() {
                Token::Struct => self.struct_def().map(|s| {
                    structs.push(s);
                }),
                Token::Use => self.use_decl().map(|u| {
                    uses.push(u);
                }),
                _ => self.func_def().map(|f| {
                    funcs.push(f);
                }),
            };
            if let Err(e) = result {
                self.errors.push(e);
                self.sync_top();
            }
            if self.pos == before {
                // recovery made no progress; force it so parsing terminates
                self.advance();
            }
        }
        (Program { uses, structs, funcs }, self.errors)
    }

    /// top-level recovery: skip to the next plausible item boundary
    fn sync_top(&mut self) {
        while !matches!(
            self.peek(),
            Token::Eof | Token::Fn | Token::Struct | Token::Use
        ) {
            self.advance();
        }
    }

    /// statement-level recovery: skip to a probable statement boundary —
    /// the next `;` (consumed), `}`/Eof, or a token that can start a statement
    fn sync_stmt(&mut self) {
        loop {
            match self.peek() {
                Token::Semicolon => {
                    self.advance();
                    return;
                }
                Token::RBrace | Token::Eof => return,
                Token::Let
                | Token::If
                | Token::While
                | Token::For
                | Token::Return
                | Token::Break
                | Token::Continue
                | Token::Defer
                | Token::Arena
                | Token::LBrace
                | Token::Ident(_) => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    /// struct Name { field: Type, ... }
    fn struct_def(&mut self) -> PResult<StructDef> {
        let start = self.peek_span();
        self.expect(&Token::Struct)?;
        let name = self.ident()?;
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        while *self.peek() != Token::RBrace {
            let fname = self.ident()?;
            self.expect(&Token::Colon)?;
            let ty = self.type_expr()?;
            fields.push((fname, ty));
            // comma optional before '}'
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(StructDef {
            name,
            fields,
            span: self.span_from(start),
            file: String::new(),
        })
    }

    /// use std;   |   use "lib/math.wl";
    fn use_decl(&mut self) -> PResult<UseDecl> {
        let start = self.peek_span();
        self.expect(&Token::Use)?;
        let path = match self.peek().clone() {
            Token::Str(s) => {
                self.advance();
                Some(s)
            }
            Token::Ident(i) if i == "std" => {
                self.advance();
                None
            }
            t => {
                let sp = self.peek_span();
                return err_at(sp, format!("expected \"path\" or std, found {:?}", t));
            }
        };
        self.expect(&Token::Semicolon)?;
        Ok(UseDecl {
            path,
            resolved: None,
            span: self.span_from(start),
        })
    }

    /// Type := '*'* Ident | '[' Type ';' INT ']'
    fn type_expr(&mut self) -> PResult<String> {
        if self.eat(&Token::LBracket) {
            let inner = self.type_expr()?;
            self.expect(&Token::Semicolon)?;
            let n = match self.peek().clone() {
                Token::Int(v) => {
                    self.advance();
                    v
                }
                t => {
                    let sp = self.peek_span();
                    return err_at(sp, format!("expected array length, found {:?}", t));
                }
            };
            self.expect(&Token::RBracket)?;
            return Ok(format!("[{}; {}]", inner, n));
        }
        let mut stars = String::new();
        while self.eat(&Token::Star) {
            stars.push('*');
        }
        let base = match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                s
            }
            t => {
                let sp = self.peek_span();
                return err_at(sp, format!("expected type, found {:?}", t));
            }
        };
        Ok(format!("{}{}", stars, base))
    }

    fn func_def(&mut self) -> PResult<FuncDef> {
        let start = self.peek_span();
        self.expect(&Token::Fn)?;
        let name = self.ident()?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if *self.peek() != Token::RParen {
            loop {
                let pstart = self.peek_span();
                let pname = self.ident()?;
                let ty = if self.eat(&Token::Colon) {
                    Some(self.type_name()?)
                } else {
                    None
                };
                params.push(Param {
                    name: pname,
                    ty,
                    span: self.span_from(pstart),
                });
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
        Ok(FuncDef {
            name,
            params,
            ret_type,
            body,
            span: self.span_from(start),
            file: String::new(),
        })
    }

    fn type_name(&mut self) -> PResult<String> {
        self.type_expr()
    }

    fn ident(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Token::Ident(s) => {
                self.advance();
                Ok(s)
            }
            t => {
                let sp = self.peek_span();
                err_at(sp, format!("expected identifier, found {:?}", t))
            }
        }
    }

    fn block(&mut self) -> PResult<Block> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        while *self.peek() != Token::RBrace && *self.peek() != Token::Eof {
            let before = self.pos;
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(e) => {
                    // record and resync; parsing continues inside this block
                    self.errors.push(e);
                    self.sync_stmt();
                    if self.pos == before {
                        self.advance();
                    }
                }
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Block(stmts))
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let start = self.peek_span();
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
                Ok(self.node(start, StmtKind::Let(name, ty, e)))
            }
            // bare `x := expr;` — inference-first declaration (canonical form)
            _ if matches!(self.peek(), Token::Ident(_))
                && matches!(self.peek_at(1), Token::Define) =>
            {
                let name = self.ident()?;
                self.advance(); // :=
                let e = self.expr(0)?;
                self.expect(&Token::Semicolon)?;
                Ok(self.node(start, StmtKind::Let(name, None, e)))
            }
            Token::If => {
                let i = self.if_stmt()?;
                Ok(self.node(start, StmtKind::If(i)))
            }
            Token::While => {
                self.advance();
                let cond = self.expr(0)?;
                let body = self.block()?;
                Ok(self.node(start, StmtKind::While(cond, body)))
            }
            Token::For => {
                self.advance();
                let var = self.ident()?;
                if !self.eat(&Token::In) {
                    return err_at(self.peek_span(), "expected 'in' after for loop variable");
                }
                let start_e = self.expr(0)?;
                self.expect(&Token::DotDot)?;
                let end = self.expr(0)?;
                let body = self.block()?;
                Ok(self.node(start, StmtKind::For(var, start_e, end, body)))
            }
            Token::Break => {
                self.advance();
                self.expect(&Token::Semicolon)?;
                Ok(self.node(start, StmtKind::Break))
            }
            Token::Continue => {
                self.advance();
                self.expect(&Token::Semicolon)?;
                Ok(self.node(start, StmtKind::Continue))
            }
            Token::LBrace => {
                let b = self.block()?;
                Ok(self.node(start, StmtKind::Block(b)))
            }
            Token::Arena => {
                self.advance();
                let cap = if self.eat(&Token::LParen) {
                    let c = self.expr(0)?;
                    self.expect(&Token::RParen)?;
                    Some(c)
                } else {
                    None
                };
                let body = self.block()?;
                Ok(self.node(start, StmtKind::Arena(cap, body)))
            }
            Token::Defer => {
                self.advance();
                // `defer { ... }` — multi-statement defer (no expression
                // starts with `{`, so the lookahead is unambiguous)
                if *self.peek() == Token::LBrace {
                    let b = self.block()?;
                    Ok(self.node(start, StmtKind::Defer(DeferBody::Block(b))))
                } else {
                    let e = self.expr(0)?;
                    self.expect(&Token::Semicolon)?;
                    Ok(self.node(start, StmtKind::Defer(DeferBody::Expr(e))))
                }
            }
            Token::Return => {
                self.advance();
                let e = if *self.peek() == Token::Semicolon {
                    None
                } else {
                    Some(self.expr(0)?)
                };
                self.expect(&Token::Semicolon)?;
                Ok(self.node(start, StmtKind::Return(e)))
            }
            _ => {
                let e = self.expr(0)?;
                let compound = match self.peek() {
                    Token::Assign => Some(CompoundOp::Set),
                    Token::PlusEq => Some(CompoundOp::Add),
                    Token::MinusEq => Some(CompoundOp::Sub),
                    Token::StarEq => Some(CompoundOp::Mul),
                    Token::SlashEq => Some(CompoundOp::Div),
                    Token::PercentEq => Some(CompoundOp::Mod),
                    _ => None,
                };
                if let Some(op) = compound {
                    self.advance();
                    let value = self.expr(0)?;
                    self.expect(&Token::Semicolon)?;
                    return Ok(self.node(
                        start,
                        StmtKind::Assign(AssignStmt {
                            target: e,
                            value,
                            op,
                        }),
                    ));
                }
                self.expect(&Token::Semicolon)?;
                Ok(self.node(start, StmtKind::ExprStmt(e)))
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
            Token::As => Some(20),
            Token::OrOr => Some(2),
            Token::AndAnd => Some(4),
            Token::BitOr => Some(6),
            Token::BitXor => Some(8),
            Token::Amp => Some(10),
            Token::Eq | Token::NotEq | Token::Lt | Token::Gt | Token::LtEq | Token::GtEq => Some(12),
            Token::Shl | Token::Shr => Some(14),
            Token::Plus | Token::Minus => Some(16),
            Token::Star | Token::Slash | Token::Percent => Some(18),
            _ => None,
        }
    }

    fn binop(t: &Token) -> BinOp {
        match t {
            Token::OrOr => BinOp::Or,
            Token::AndAnd => BinOp::And,
            Token::BitOr => BinOp::BitOr,
            Token::BitXor => BinOp::BitXor,
            Token::Amp => BinOp::BitAnd,
            Token::Shl => BinOp::Shl,
            Token::Shr => BinOp::Shr,
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
        while let Some(bp) = Self::lbp(self.peek()) {
            if bp < min_bp {
                break;
            }
            if *self.peek() == Token::As {
                let start = lhs.span;
                self.advance();
                let ty = self.type_expr()?;
                lhs = self.node(start, ExprKind::Cast(ty, Box::new(lhs)));
                continue;
            }
            let op = Self::binop(self.peek());
            self.advance();
            let rhs = self.expr(bp + 1)?;
            let start = lhs.span;
            lhs = self.node(start, ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        let start = self.peek_span();
        if self.eat(&Token::Minus) {
            let inner = self.unary()?;
            return Ok(self.node(start, ExprKind::Unary(UnOp::Neg, Box::new(inner))));
        }
        if self.eat(&Token::Bang) {
            let inner = self.unary()?;
            return Ok(self.node(start, ExprKind::Unary(UnOp::Not, Box::new(inner))));
        }
        if self.eat(&Token::BitNot) {
            let inner = self.unary()?;
            return Ok(self.node(start, ExprKind::Unary(UnOp::BitNot, Box::new(inner))));
        }
        if self.eat(&Token::Amp) {
            let inner = self.unary()?;
            return Ok(self.node(start, ExprKind::AddrOf(Box::new(inner))));
        }
        if self.eat(&Token::Star) {
            let inner = self.unary()?;
            return Ok(self.node(start, ExprKind::Deref(Box::new(inner))));
        }
        let p = self.primary()?;
        self.postfix(p)
    }

    /// postfix: field access chains (a.b.c) after calls/literals
    fn postfix(&mut self, e: Expr) -> PResult<Expr> {
        let mut cur = e;
        loop {
            if self.eat(&Token::Dot) {
                let f = match self.ident() {
                    Ok(f) => f,
                    Err(_) => break,
                };
                let start = cur.span;
                cur = self.node(start, ExprKind::Field(Box::new(cur), f));
                continue;
            }
            if self.eat(&Token::LBracket) {
                let idx = self.expr(0)?;
                self.expect(&Token::RBracket)?;
                let start = cur.span;
                cur = self.node(start, ExprKind::Index(Box::new(cur), Box::new(idx)));
                continue;
            }
            break;
        }
        Ok(cur)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let start = self.peek_span();
        let t = self.peek().clone();
        match t {
            Token::Int(v) => {
                self.advance();
                Ok(self.node(start, ExprKind::Int(v)))
            }
            // `10u8` desugars to a cast so width conversion stays explicit
            // in the AST and the checker treats it exactly like `10 as u8`
            Token::IntSuf(v, suf) => {
                self.advance();
                let lit = if v <= i64::MAX as u64 {
                    self.node(start, ExprKind::Int(v as i64))
                } else {
                    self.node(start, ExprKind::UInt(v))
                };
                Ok(self.node(start, ExprKind::Cast(suf, Box::new(lit))))
            }
            Token::Float(v) => {
                self.advance();
                Ok(self.node(start, ExprKind::Float(v)))
            }
            Token::FloatSuf(v, suf) => {
                self.advance();
                let lit = self.node(start, ExprKind::Float(v));
                Ok(self.node(start, ExprKind::Cast(suf, Box::new(lit))))
            }
            Token::Str(s) => {
                self.advance();
                Ok(self.node(start, ExprKind::Str(s)))
            }
            Token::True => {
                self.advance();
                Ok(self.node(start, ExprKind::Bool(true)))
            }
            Token::False => {
                self.advance();
                Ok(self.node(start, ExprKind::Bool(false)))
            }
            Token::Ident(name) => {
                self.advance();
                let mut qname = name;
                while self.eat(&Token::DoubleColon) {
                    let part = self.ident()?;
                    qname = format!("{}::{}", qname, part);
                }
                // wlel_sizeof(Type) — type form
                if qname == "wlel_sizeof" && self.eat(&Token::LParen) {
                    let ty = self.type_expr()?;
                    self.expect(&Token::RParen)?;
                    return self.postfix(self.node(start, ExprKind::Sizeof(ty)));
                }
                // new(T) / new(T, count) — arena-aware typed allocation
                if qname == "new" && self.eat(&Token::LParen) {
                    let ty = self.type_expr()?;
                    let count = if self.eat(&Token::Comma) {
                        Some(Box::new(self.expr(0)?))
                    } else {
                        None
                    };
                    self.expect(&Token::RParen)?;
                    return self.postfix(self.node(start, ExprKind::New(ty, count)));
                }
                // wlel_arena() — current arena pointer
                if qname == "wlel_arena" && self.eat(&Token::LParen) {
                    self.expect(&Token::RParen)?;
                    return self.postfix(self.node(start, ExprKind::CurrentArena));
                }
                let name = qname;
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
                    return self.postfix(self.node(start, ExprKind::Call(name, args)));
                }
                // struct literal? Name { Field: expr, ... } — require '{ IDENT :' so
                // statement blocks like `while i < n { i = ...` never collide
                if *self.peek() == Token::LBrace
                    && matches!(self.peek_at(1), Token::Ident(_))
                    && matches!(self.peek_at(2), Token::Colon)
                {
                    self.advance(); // {
                    let mut fields = Vec::new();
                    while *self.peek() != Token::RBrace {
                        let fname = self.ident()?;
                        self.expect(&Token::Colon)?;
                        let v = self.expr(0)?;
                        fields.push((fname, v));
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                    self.expect(&Token::RBrace)?;
                    return self.postfix(self.node(start, ExprKind::StructLit(name, fields)));
                }
                self.postfix(self.node(start, ExprKind::Ident(name)))
            }
            Token::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                if *self.peek() != Token::RBracket {
                    loop {
                        elems.push(self.expr(0)?);
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&Token::RBracket)?;
                self.postfix(self.node(start, ExprKind::ArrayLit(elems)))
            }
            Token::LParen => {
                self.advance();
                let e = self.expr(0)?;
                self.expect(&Token::RParen)?;
                Ok(e)
            }
            other => err_at(start, format!("unexpected token {:?}", other)),
        }
    }
}
