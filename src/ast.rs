use crate::span::{Span, Spanned};

#[derive(Debug, Clone)]
pub struct Program {
    pub uses: Vec<UseDecl>,
    pub structs: Vec<StructDef>,
    /// tagged enums: `enum Shape { Circle(float), Point }`
    pub enums: Vec<EnumDef>,
    /// `impl Name[T, ...] { fn method(self, ...) { ... } }` blocks —
    /// desugared by the checker into plain functions (`Name__method`)
    pub impls: Vec<ImplDef>,
    pub funcs: Vec<FuncDef>,
    /// `test "name" { ... }` blocks, merged from this file and its imports
    pub tests: Vec<TestDef>,
}
#[derive(Debug, Clone)]
pub struct UseDecl {
    /// None = std
    pub path: Option<String>,
    /// canonical path of a file import, filled in by the loader so the
    /// checker can attribute usage to this import (None in unit tests)
    pub resolved: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    /// raw type strings, e.g. "float", "*Point" — may reference the struct's
    /// type parameters ("T") when type_params is non-empty
    pub fields: Vec<(String, String)>,
    /// generic type parameters ("T", "K", ...) — empty for a plain struct
    pub type_params: Vec<String>,
    pub span: Span,
    /// absolute path of the source file this struct was parsed from
    /// (used for unused-import detection)
    pub file: String,
}

/// one variant of a tagged enum: `Circle(f64)` or bare `Point`
#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: String,
    /// raw payload type strings, in order ("f64", "*Point", ...)
    pub payloads: Vec<String>,
}

/// `enum Name { Variant(T, ...), ... }` — a closed tagged union. The C
/// backend lowers it to a struct with an `int _tag` discriminant plus a
/// union of per-variant payload structs; `match` is the only way to
/// destructure it.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<VariantDef>,
    /// generic type parameters ("T", "E", ...) — empty for a plain enum;
    /// generic enums are monomorphized per concrete argument list
    pub type_params: Vec<String>,
    pub span: Span,
    /// absolute path of the source file this enum was parsed from
    /// (used for unused-import detection)
    pub file: String,
}

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_type: Option<String>,
    pub body: Block,
    /// generic type parameters ("T") — empty for a plain function;
    /// generic functions are monomorphized per concrete type at call sites
    pub type_params: Vec<String>,
    /// `extern fn ...;` — declaration of a C function; no body is emitted,
    /// the call links against the external symbol at cc time
    pub is_extern: bool,
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

/// `impl Name[T, ...] { fn method(self, ...) { ... } }` — a group of methods
/// attached to a struct or enum. The checker lowers every method into a
/// plain function named `Name__method` whose first parameter is the
/// receiver; `recv.method(args)` resolves through the method table and
/// rewrites into that call.
#[derive(Debug, Clone)]
pub struct ImplDef {
    /// the struct/enum being implemented (source name: "Pt", "Vec")
    pub type_name: String,
    /// type parameters of the impl — must repeat the type's own parameters
    /// verbatim for a generic type, and be empty for a concrete one
    pub type_params: Vec<String>,
    pub methods: Vec<FuncDef>,
    pub span: Span,
    /// absolute path of the source file this impl was parsed from
    /// (used for unused-import detection)
    pub file: String,
}

/// `test "name" { ... }` — a void body run by `wlel test`; a failed
/// `assert`/`assert_eq` aborts the test and is reported with file:line
#[derive(Debug, Clone)]
pub struct TestDef {
    pub name: String,
    pub body: Block,
    pub span: Span,
    /// absolute path of the source file this test was parsed from
    pub file: String,
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
    /// for var in collection { body } — iterates [T; N] arrays and Vec[T];
    /// the loop variable is a copy of each element
    ForIn(ForInStmt),
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
    /// match expr { ... } as a statement: arms run for effect, values
    /// discarded; still exhaustiveness-checked
    Match(MatchStmt),
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

/// `for x in coll { ... }` — `elem` (element type name) is filled in by the
/// checker so the codegen can declare the loop variable with an exact C type
#[derive(Debug, Clone, PartialEq)]
pub struct ForInStmt {
    pub var: String,
    pub iter: Expr,
    pub body: Block,
    pub elem: Option<String>,
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

/// `match scrutinee { Pattern => body, ... }` — destructures a tagged enum.
/// As a value (`let x := match ...` / `return match ...`) every arm is an
/// expression and all arms share one type (`result_ty`, filled by the
/// checker); as a statement arms may also be blocks and values are dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchStmt {
    pub scrutinee: Expr,
    pub arms: Vec<MatchArm>,
    /// unified arm value type in value form (canonical type name); None in
    /// statement form
    pub result_ty: Option<String>,
    /// checker-filled: every variant is covered AND every arm exits the
    /// function, so code following the match is unreachable
    pub all_arms_terminate: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: MatchBody,
    pub span: Span,
}

/// `VariantName(bind1, bind2)` destructures the payload into fresh
/// constants; `_` matches anything. `tag` (the variant's declaration
/// index) and binding types are filled in by the checker.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Variant {
        name: String,
        tag: usize,
        binds: Vec<MatchBinding>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchBinding {
    pub name: String,
    pub ty: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchBody {
    Expr(Expr),
    Block(Block),
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
    /// name(args)  |  name[T, ...](args) — explicit generic instantiation
    /// (type args may also be filled in by the checker when rewriting calls
    /// to monomorphized/builtin helpers)
    Call(String, Vec<String>, Vec<Expr>),
    /// &lvalue
    AddrOf(Box<Expr>),
    /// *ptr
    Deref(Box<Expr>),
    /// obj.field (auto-derefs one pointer level, decided by the checker)
    Field(Box<Expr>, String),
    /// recv.method(args) — resolved by the checker into a call to the
    /// impl's desugared function (`Pt__len(recv, ...)`); the receiver
    /// adapts to the declared self form (auto-deref / auto-address)
    MethodCall(Box<Expr>, String, Vec<Expr>),
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
    /// match expr { ... } in value position — only directly as the
    /// initializer of a let or as a return value (the checker rejects it
    /// anywhere else; codegen hoists it to statements)
    Match(Box<MatchStmt>),
    /// Variant(payloads...) — an enum value constructor, rewritten by the
    /// checker from a call to a variant name; enum is the (possibly
    /// mangled) enum type, tag the variant index, payload the variant name
    EnumLit(String, usize, String, Vec<Expr>),
    /// expr? — the try operator on a Result/Option: propagate the error
    /// variant out of the enclosing function, yield the success payload.
    /// Only directly as the value of a let/assignment/return or as a
    /// statement (the checker rejects it anywhere else; codegen hoists it).
    /// ret_name/err_tag/err_variant/err_arity/ok_variant are filled in by
    /// the checker from the two enum instances involved
    Try(TryExpr),
}

/// the checker-validated shape of `expr?`: `inner` is a std Result[T, E] or
/// Option[T]; the enclosing function returns a Result/Option sharing the
/// same error variant, whose instance is `ret_name`
#[derive(Debug, Clone, PartialEq)]
pub struct TryExpr {
    pub inner: Box<Expr>,
    /// mangled enum instance of the enclosing function's return type
    pub ret_name: String,
    /// tag of the error variant ("Err" / "None") in both instances
    pub err_tag: usize,
    pub err_variant: String,
    /// payload count of the error variant (0 for Option::None)
    pub err_arity: usize,
    /// success variant name of the inner enum ("Ok" / "Some")
    pub ok_variant: String,
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
