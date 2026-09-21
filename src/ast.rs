use crate::span::{Span, Spanned};

#[derive(Debug)]
pub struct Program {
    pub uses: Vec<UseDecl>,
    pub structs: Vec<StructDef>,
    pub funcs: Vec<FuncDef>,
}
#[derive(Debug)]
pub struct UseDecl {
    /// None = std
    pub path: Option<String>,
    /// canonical path of a file import, filled in by the loader so the
    /// checker can attribute usage to this import (None in unit tests)
    pub resolved: Option<String>,
    pub span: Span,
}

#[derive(Debug)]
pub struct StructDef {
    pub name: String,
    /// raw type strings, e.g. "float", "*Point"
    pub fields: Vec<(String, String)>,
    pub span: Span,
    /// absolute path of the source file this struct was parsed from
    /// (used for unused-import detection)
    pub file: String,
}

#[derive(Debug)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_type: Option<String>,
    pub body: Block,
    pub span: Span,
    /// absolute path of the source file this function was parsed from
    /// (used for `#line` directives so cc errors map back to `.wl`)
    pub file: String,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block(pub Vec<Stmt>);

pub type Stmt = Spanned<StmtKind>;

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    /// x := expr;        (inferred)
    /// let x: T = expr;  (annotated)
    Let(String, Option<String>, Expr),
    Assign(AssignStmt),
    If(IfStmt),
    While(Expr, Block),
    /// for var in start..end { body }
    For(String, Expr, Expr, Block),
    Break,
    Continue,
    /// return; | return expr;
    Return(Option<Expr>),
    /// foo(...);
    ExprStmt(Expr),
    /// runs at scope exit (LIFO): `defer expr;` or `defer { stmt; ... }`
    Defer(DeferBody),
    /// arena(bytes) { ... } — scoped scratchpad, O(1) free at exit
    Arena(Option<Expr>, Block),
    /// { ... } nested scope
    Block(Block),
}

/// validated by the checker into one of: variable, field path, deref
#[derive(Debug, Clone, PartialEq)]
pub struct AssignStmt {
    pub target: Expr,
    pub value: Expr,
    pub op: CompoundOp,
}

/// the two forms of `defer`: a single void expression or a block of statements
#[derive(Debug, Clone, PartialEq)]
pub enum DeferBody {
    Expr(Expr),
    Block(Block),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IfStmt {
    pub cond: Expr,
    pub then_body: Block,
    pub else_branch: Option<ElseBranch>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElseBranch {
    If(Box<IfStmt>),
    Block(Block),
}

pub type Expr = Spanned<ExprKind>;

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Int(i64),
    /// integer literal above i64::MAX (only via a u-suffixed literal)
    UInt(u64),
    Float(f64),
    Str(String),
    Bool(bool),
    Ident(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    /// &lvalue
    AddrOf(Box<Expr>),
    /// *ptr
    Deref(Box<Expr>),
    /// obj.field (auto-derefs one pointer level, decided by the checker)
    Field(Box<Expr>, String),
    /// Point { x: 1.0, y: 2.0 }
    StructLit(String, Vec<(String, Expr)>),
    /// expr as T
    Cast(String, Box<Expr>),
    /// a[i]
    Index(Box<Expr>, Box<Expr>),
    /// [1, 2, 3]
    ArrayLit(Vec<Expr>),
    /// wlel_sizeof(T)
    Sizeof(String),
    /// new(T) or new(T, count) — arena-aware typed allocation
    New(String, Option<Box<Expr>>),
    /// wlel_arena() — pointer to the innermost active arena
    CurrentArena,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}
