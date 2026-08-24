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
}

#[derive(Debug)]
pub struct StructDef {
    pub name: String,
    /// raw type strings, e.g. "float", "*Point"
    pub fields: Vec<(String, String)>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct Block(pub Vec<Stmt>);

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// x := expr;        (inferred)
    /// let x: T = expr;  (annotated)
    Let(String, Option<String>, Expr),
    Assign(AssignStmt),
    If(IfStmt),
    While(Expr, Block),
    /// return; | return expr;
    Return(Option<Expr>),
    /// foo(...);
    ExprStmt(Expr),
    /// runs at scope exit (LIFO)
    Defer(Expr),
    /// { ... } nested scope
    Block(Block),
}

/// validated by the checker into one of: variable, field path, deref
#[derive(Debug, Clone, PartialEq)]
pub struct AssignStmt {
    pub target: Expr,
    pub value: Expr,
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
