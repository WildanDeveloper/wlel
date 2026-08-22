#[derive(Debug)]
pub struct Program {
    pub funcs: Vec<FuncDef>,
}

#[derive(Debug)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_type: Option<String>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct Block(pub Vec<Stmt>);

#[derive(Debug, PartialEq)]
pub enum Stmt {
    /// x := expr;
    Let(String, Expr),
    /// x = expr;
    Assign(String, Expr),
    If(IfStmt),
    While(Expr, Block),
    /// return; | return expr;
    Return(Option<Expr>),
    /// foo(...);
    ExprStmt(Expr),
}

#[derive(Debug, PartialEq)]
pub struct IfStmt {
    pub cond: Expr,
    pub then_body: Block,
    pub else_branch: Option<ElseBranch>,
}

#[derive(Debug, PartialEq)]
pub enum ElseBranch {
    If(Box<IfStmt>),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Ident(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}
