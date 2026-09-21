use crate::ast::*;
use crate::span::{Span, Spanned};
use std::collections::{HashMap, HashSet};

/// Integer width. `int` in source is an alias for `I64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntW {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Usize,
}

impl IntW {
    /// canonical source name; `I64` keeps the friendly `int` alias
    pub fn name(self) -> &'static str {
        match self {
            IntW::I8 => "i8",
            IntW::I16 => "i16",
            IntW::I32 => "i32",
            IntW::I64 => "int",
            IntW::U8 => "u8",
            IntW::U16 => "u16",
            IntW::U32 => "u32",
            IntW::U64 => "u64",
            IntW::Usize => "usize",
        }
    }

    fn bits(self) -> u32 {
        match self {
            IntW::I8 | IntW::U8 => 8,
            IntW::I16 | IntW::U16 => 16,
            IntW::I32 | IntW::U32 => 32,
            IntW::I64 | IntW::U64 | IntW::Usize => 64,
        }
    }

    fn signed(self) -> bool {
        matches!(self, IntW::I8 | IntW::I16 | IntW::I32 | IntW::I64)
    }

    /// does the integer literal value fit in this width?
    /// `neg` marks a negated literal (`-5`); unsigned widths reject negatives.
    fn fits_literal(self, v: i64, neg: bool) -> bool {
        if self.signed() {
            let (lo, hi): (i64, i64) = match self {
                IntW::I8 => (-128, 127),
                IntW::I16 => (-32_768, 32_767),
                IntW::I32 => (-2_147_483_648, 2_147_483_647),
                _ => (i64::MIN, i64::MAX),
            };
            if neg {
                -v >= lo && -v <= hi
            } else {
                v >= lo && v <= hi
            }
        } else {
            let hi: u64 = match self {
                IntW::U8 => 255,
                IntW::U16 => 65_535,
                IntW::U32 => 4_294_967_295,
                // u64/usize: every non-negative i64 fits
                _ => u64::MAX,
            };
            !neg && v >= 0 && (v as u64) <= hi
        }
    }

    /// C arithmetic is exact for widths >= 32 bits; smaller widths are
    /// promoted to `int`, so generated code must truncate explicitly
    fn needs_wrap_cast(self) -> bool {
        self.bits() < 32
    }
}

/// Float width. `float` in source is an alias for `F64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatW {
    F32,
    F64,
}

impl FloatW {
    pub fn name(self) -> &'static str {
        match self {
            FloatW::F32 => "f32",
            FloatW::F64 => "float",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int(IntW),
    Float(FloatW),
    Bool,
    Str,
    Void,
    Struct(String),
    Ptr(Box<Type>),
    Array(Box<Type>, usize),
}

impl Type {
    fn from_builtin(name: &str) -> Option<Type> {
        match name {
            "int" | "i64" => Some(Type::Int(IntW::I64)),
            "i8" => Some(Type::Int(IntW::I8)),
            "i16" => Some(Type::Int(IntW::I16)),
            "i32" => Some(Type::Int(IntW::I32)),
            "u8" | "byte" | "char" => Some(Type::Int(IntW::U8)),
            "u16" => Some(Type::Int(IntW::U16)),
            "u32" => Some(Type::Int(IntW::U32)),
            "u64" => Some(Type::Int(IntW::U64)),
            "usize" => Some(Type::Int(IntW::Usize)),
            "float" | "f64" => Some(Type::Float(FloatW::F64)),
            "f32" => Some(Type::Float(FloatW::F32)),
            "bool" => Some(Type::Bool),
            "string" => Some(Type::Str),
            "void" => Some(Type::Void),
            _ => None,
        }
    }

    fn name(&self) -> String {
        match self {
            Type::Int(w) => w.name().into(),
            Type::Float(w) => w.name().into(),
            Type::Bool => "bool".into(),
            Type::Str => "string".into(),
            Type::Void => "void".into(),
            Type::Struct(n) => n.clone(),
            Type::Ptr(t) => format!("*{}", t.name()),
            Type::Array(t, n) => format!("[{}; {}]", t.name(), n),
        }
    }
}

fn is_int(t: &Type) -> bool {
    matches!(t, Type::Int(_))
}

fn is_float_t(t: &Type) -> bool {
    matches!(t, Type::Float(_))
}

fn is_num(t: &Type) -> bool {
    is_int(t) || is_float_t(t)
}

fn int_width(t: &Type) -> Option<IntW> {
    match t {
        Type::Int(w) => Some(*w),
        _ => None,
    }
}

fn float_width(t: &Type) -> Option<FloatW> {
    match t {
        Type::Float(w) => Some(*w),
        _ => None,
    }
}

/// value of an untyped literal, for range checks
enum LitVal {
    Int(i64, bool), // (value, negated)
    Float,
}

fn lit_val(e: &Expr) -> Option<LitVal> {
    match &e.node {
        ExprKind::Int(v) => Some(LitVal::Int(*v, false)),
        ExprKind::Float(_) => Some(LitVal::Float),
        ExprKind::Unary(UnOp::Neg, x) => match &x.node {
            ExprKind::Int(v) => Some(LitVal::Int(*v, true)),
            _ => None,
        },
        _ => None,
    }
}

/// true when the expression is an unsuffixed (width-flexible) literal
fn is_untyped_lit(e: &Expr) -> bool {
    lit_val(e).is_some()
}

#[derive(Clone)]
struct FuncSig {
    params: Vec<Type>,
    ret: Type,
}

/// a declared local/parameter, for unused-symbol warnings
#[derive(Clone)]
struct VarInfo {
    ty: Type,
    read: bool,
    span: Span,
}

#[derive(Debug)]
pub struct CheckError {
    pub msg: String,
    pub span: Span,
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.span, self.msg)
    }
}

impl std::error::Error for CheckError {}

/// Non-fatal diagnostic: never fails the build, only reported.
#[derive(Debug)]
pub struct CheckWarning {
    pub msg: String,
    pub span: Span,
}

impl std::fmt::Display for CheckWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: warning: {}", self.span, self.msg)
    }
}

type CResult<T> = Result<T, CheckError>;

fn err_at<T>(span: Span, msg: impl Into<String>) -> CResult<T> {
    Err(CheckError { msg: msg.into(), span })
}

pub struct Checker {
    sigs: HashMap<String, FuncSig>,
    structs: HashMap<String, Vec<(String, Type)>>,
    scopes: Vec<HashMap<String, VarInfo>>,
    current_ret: Type,
    loop_depth: usize,
    arena_depth: usize,
    use_std: bool,
    std_called: bool,
    /// function names referenced by calls (for unused-import detection)
    called_funcs: HashSet<String>,
    /// struct names referenced anywhere (types, literals, casts)
    used_structs: HashSet<String>,
    /// positive while checking a `defer { ... }` block: control flow out of
    /// it is forbidden, because the block runs during scope unwinding
    defer_depth: usize,
    /// statements after a return/break/continue in the current block
    block_terminated: bool,
    warnings: Vec<CheckWarning>,
}

impl Checker {
    /// Passes: (0) structs, (1) function signatures, (2) bodies.
    /// Inferred `let` annotations are written back into the AST so the
    /// codegen can emit exact C types. Auto-deref on field access is
    /// rewritten into the AST here as well.
    /// Returns non-fatal warnings; an Err still means the program is rejected.
    pub fn check(program: &mut Program) -> CResult<Vec<CheckWarning>> {
        let use_std = program.uses.iter().any(|u| u.path.is_none());
        let mut cx = Checker {
            sigs: HashMap::new(),
            structs: HashMap::new(),
            scopes: vec![HashMap::new()],
            current_ret: Type::Void,
            loop_depth: 0,
            arena_depth: 0,
            use_std,
            std_called: false,
            called_funcs: HashSet::new(),
            used_structs: HashSet::new(),
            defer_depth: 0,
            block_terminated: false,
            warnings: Vec::new(),
        };

        // pass 0: register struct names, then resolve field types
        for st in &program.structs {
            if cx.structs.insert(st.name.clone(), Vec::new()).is_some() {
                return err_at(st.span, format!("duplicate struct '{}'", st.name));
            }
        }
        for st in &program.structs {
            let mut fields = Vec::with_capacity(st.fields.len());
            for (fname, fty) in &st.fields {
                let t = cx.resolve_type_str(fty, st.span)?;
                if t == Type::Void {
                    return err_at(
                        st.span,
                        format!("struct '{}': field '{}' cannot be void", st.name, fname),
                    );
                }
                fields.push((fname.clone(), t));
            }
            cx.structs.insert(st.name.clone(), fields);
        }

        // pass 1: signatures
        for f in &program.funcs {
            let ret = cx.resolve_type_str(f.ret_type.as_deref().unwrap_or("void"), f.span)?;
            if matches!(ret, Type::Array(..)) {
                return err_at(
                    f.span,
                    format!(
                        "function '{}': cannot return an array — return a pointer or wrap it in a struct",
                        f.name
                    ),
                );
            }
            let mut params = Vec::with_capacity(f.params.len());
            for p in &f.params {
                let t = cx.resolve_type_str(p.ty.as_deref().unwrap_or("int"), p.span)?;
                if t == Type::Void {
                    return err_at(
                        p.span,
                        format!(
                            "function '{}': parameter '{}' cannot be void",
                            f.name, p.name
                        ),
                    );
                }
                if matches!(t, Type::Array(..)) {
                    return err_at(
                        p.span,
                        format!(
                            "function '{}': parameter '{}' — pass arrays by pointer or wrap them in a struct",
                            f.name, p.name
                        ),
                    );
                }
                params.push(t);
            }
            if cx.sigs.insert(f.name.clone(), FuncSig { params, ret }).is_some() {
                return err_at(f.span, format!("duplicate function '{}'", f.name));
            }
        }

        // pass 2: bodies
        for f in program.funcs.iter_mut() {
            let sig = cx.sigs.get(&f.name).unwrap().clone();
            let sig_params = sig.params.clone();
            let sig_ret = sig.ret.clone();
            cx.current_ret = sig_ret.clone();
            cx.scopes = vec![HashMap::new()];
            cx.block_terminated = false;
            for (p, t) in f.params.iter().zip(sig_params.iter()) {
                cx.declare(&f.name, &p.name, t.clone(), p.span)?;
            }
            cx.check_block_mut(&mut f.body)?;
            if sig_ret != Type::Void && !guarantees_return(&f.body) {
                return err_at(
                    f.span,
                    format!(
                        "function '{}' declares '-> {}' but has no return statement",
                        f.name,
                        sig_ret.name()
                    ),
                );
            }
            // params live in scope 0: warn about the never-read ones
            cx.report_unused_scope("parameter");
            cx.scopes[0].clear();
        }

        // unused imports: a file import is used when any function or struct
        // defined in it is referenced; `use std` when any std:: call exists
        let used_files: HashSet<&str> = called_files(program, &cx.called_funcs, &cx.used_structs);
        for u in &program.uses {
            if let (Some(orig), Some(resolved)) = (&u.path, &u.resolved) {
                if !used_files.contains(resolved.as_str()) {
                    cx.warnings.push(CheckWarning {
                        msg: format!("import '{orig}' is never used"),
                        span: u.span,
                    });
                }
            }
        }
        if use_std && !cx.std_called {
            if let Some(u) = program.uses.iter().find(|u| u.path.is_none()) {
                cx.warnings.push(CheckWarning {
                    msg: "import 'std' is never used".into(),
                    span: u.span,
                });
            }
        }

        cx.warnings
            .sort_by_key(|w| (w.span.start.line, w.span.start.col));
        Ok(cx.warnings)
    }

    fn resolve_type_str(&mut self, s: &str, span: Span) -> CResult<Type> {
        // array form: [T; N]
        if let Some(rest) = s.strip_prefix('[') {
            let close = rest
                .rfind(']')
                .ok_or_else(|| CheckError {
                    msg: format!("bad array type '{s}'"),
                    span,
                })?;
            let inner_s = &rest[..close];
            let n: usize = inner_s
                .rsplit(';')
                .next()
                .unwrap_or("")
                .trim()
                .parse()
                .map_err(|_| CheckError {
                    msg: format!("bad array length in '{s}'"),
                    span,
                })?;
            let inner = self.resolve_type_str(
                inner_s[..inner_s.len() - inner_s.rsplit(';').next().unwrap().len()]
                    .trim_end_matches(';'),
                span,
            )?;
            if n == 0 {
                return err_at(span, format!("array length must be > 0 in '{s}'"));
            }
            return Ok(Type::Array(Box::new(inner), n));
        }
        let stars = s.chars().take_while(|c| *c == '*').count();
        let base = &s[stars..];
        let t = match Type::from_builtin(base) {
            Some(t) => t,
            None => {
                if self.structs.contains_key(base) {
                    self.used_structs.insert(base.to_string());
                    Type::Struct(base.to_string())
                } else {
                    return err_at(span, format!("unknown type '{}'", base));
                }
            }
        };
        let mut out = t;
        for _ in 0..stars {
            out = Type::Ptr(Box::new(out));
        }
        Ok(out)
    }

    fn declare(&mut self, ctx: &str, name: &str, ty: Type, span: Span) -> CResult<()> {
        if name.starts_with("_wlel_") {
            return err_at(
                span,
                format!(
                    "{ctx}: identifier '{name}' is reserved (prefix '_wlel_' is internal)"
                ),
            );
        }
        let scope = self.scopes.last_mut().unwrap();
        if scope
            .insert(name.to_string(), VarInfo { ty, read: false, span })
            .is_some()
        {
            return err_at(
                span,
                format!("{ctx}: variable '{name}' already declared in this scope"),
            );
        }
        Ok(())
    }

    fn lookup(&mut self, name: &str) -> Option<Type> {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.read = true;
                return Some(info.ty.clone());
            }
        }
        None
    }

    /// is `name` currently marked as read? (write-only assignment restore)
    fn is_read(&self, name: &str) -> Option<bool> {
        for s in self.scopes.iter().rev() {
            if let Some(info) = s.get(name) {
                return Some(info.read);
            }
        }
        None
    }

    fn mark_read(&mut self, name: &str, val: bool) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.read = val;
                return;
            }
        }
    }

    /// emit unused-symbol warnings for the innermost scope, then pop it
    fn pop_scope(&mut self) {
        self.report_unused_scope("variable");
        self.scopes.pop();
    }

    /// warn about never-read entries of the innermost scope (without popping);
    /// `_`-prefixed names are an explicit opt-out
    fn report_unused_scope(&mut self, kind: &str) {
        let unused: Vec<(String, Span)> = self
            .scopes
            .last()
            .unwrap()
            .iter()
            .filter(|(n, info)| !info.read && !n.starts_with('_'))
            .map(|(n, info)| (n.clone(), info.span))
            .collect();
        for (name, span) in unused {
            self.warnings.push(CheckWarning {
                msg: format!("unused {kind} '{name}'"),
                span,
            });
        }
    }

    fn struct_field(&self, sname: &str, field: &str, span: Span) -> CResult<Type> {
        let fields = self.structs.get(sname).ok_or_else(|| CheckError {
            msg: format!("unknown struct '{sname}'"),
            span,
        })?;
        fields
            .iter()
            .find(|(n, _)| n == field)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| CheckError {
                msg: format!("struct '{sname}' has no field '{field}'"),
                span,
            })
    }

    /// Types an expression, mutating auto-deref into Field objects.
    fn expr_ty(&mut self, e: &mut Expr) -> CResult<Type> {
        let sp = e.span;
        match &mut e.node {
            ExprKind::Int(_) => Ok(Type::Int(IntW::I64)),
            ExprKind::UInt(_) => Ok(Type::Int(IntW::U64)),
            ExprKind::Float(_) => Ok(Type::Float(FloatW::F64)),
            ExprKind::Str(_) => Ok(Type::Str),
            ExprKind::Bool(_) => Ok(Type::Bool),
            ExprKind::Ident(n) => self
                .lookup(n)
                .ok_or_else(|| CheckError {
                    msg: format!("undefined variable '{n}'"),
                    span: sp,
                }),
            ExprKind::Unary(op, x) => {
                let t = self.expr_ty(x)?;
                match op {
                    UnOp::Neg => match t {
                        Type::Int(_) | Type::Float(_) => {
                            self.wrap_small(e, &t, sp);
                            Ok(t)
                        }
                        _ => err_at(sp, format!("unary '-' needs a number, got {}", t.name())),
                    },
                    UnOp::Not => {
                        if t == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            err_at(sp, format!("'!' needs bool, got {}", t.name()))
                        }
                    }
                    UnOp::BitNot => {
                        if let Some(w) = int_width(&t) {
                            self.wrap_small(e, &t, sp);
                            Ok(Type::Int(w))
                        } else {
                            err_at(sp, format!("'~' needs int, got {}", t.name()))
                        }
                    }
                }
            }
            ExprKind::AddrOf(x) => {
                let t = self.expr_ty(x)?;
                if !is_lvalue(x) {
                    return err_at(sp, " '&' needs a variable, field or dereference");
                }
                Ok(Type::Ptr(Box::new(t)))
            }
            ExprKind::Deref(x) => {
                let t = self.expr_ty(x)?;
                match t {
                    Type::Ptr(inner) => Ok(*inner),
                    other => err_at(
                        sp,
                        format!("cannot dereference non-pointer {}", other.name()),
                    ),
                }
            }
            ExprKind::Field(obj, field) => {
                let ot = self.expr_ty(obj)?;
                // auto-deref: p.x where p: *Point becomes (*p).x
                let sname = match ot {
                    Type::Struct(n) => n,
                    Type::Ptr(inner) => match *inner {
                        Type::Struct(n) => {
                            let inner_obj = (**obj).clone();
                            **obj = Spanned::new(ExprKind::Deref(Box::new(inner_obj)), obj.span);
                            n
                        }
                        other => {
                            return err_at(
                                sp,
                                format!(
                                    "field '{field}' on pointer to non-struct {}",
                                    other.name()
                                ),
                            )
                        }
                    },
                    other => {
                        return err_at(
                            sp,
                            format!("field access '{field}' on non-struct {}", other.name()),
                        )
                    }
                };
                self.struct_field(&sname, field, sp)
            }
            ExprKind::StructLit(name, fields) => {
                let declared: Vec<(String, Type)> = match self.structs.get(name) {
                    Some(f) => f.clone(),
                    None => return err_at(sp, format!("unknown struct '{name}'")),
                };
                self.used_structs.insert(name.clone());
                if fields.len() != declared.len() {
                    return err_at(
                        sp,
                        format!(
                            "struct '{}': expected {} field(s), got {}",
                            name,
                            declared.len(),
                            fields.len()
                        ),
                    );
                }
                for (i, (fname, fexpr)) in fields.iter_mut().enumerate() {
                    if *fname != declared[i].0 {
                        return err_at(
                            sp,
                            format!(
                                "struct '{}': field {} should be '{}', found '{}'",
                                name,
                                i + 1,
                                declared[i].0,
                                fname
                            ),
                        );
                    }
                    let want = declared[i].1.clone();
                    let got = self.expr_ty(fexpr)?;
                    if !assignable_checked(&want, fexpr, &got, sp)? {
                        return err_at(
                            sp,
                            format!(
                                "struct '{}': field '{}': expected {}, got {}",
                                name,
                                fname,
                                want.name(),
                                got.name()
                            ),
                        );
                    }
                }
                Ok(Type::Struct(name.clone()))
            }
            ExprKind::Binary(op, l, r) => {
                let lt = self.expr_ty(l)?;
                let rt = self.expr_ty(r)?;
                use BinOp::*;
                match op {
                    Add | Sub | Mul | Div | Mod => {
                        if *op == Add && lt == Type::Str && rt == Type::Str {
                            let l_span = l.span;
                            let r_span = r.span;
                            let left = std::mem::replace(&mut **l, dummy_expr(l_span));
                            let right = std::mem::replace(&mut **r, dummy_expr(r_span));
                            *e = Spanned::new(
                                ExprKind::Call("_wlel_strcat".into(), vec![left, right]),
                                sp,
                            );
                            return Ok(Type::Str);
                        }
                        if is_int(&lt) && is_int(&rt) {
                            let res_w = self.int_op_width(op, l, &lt, r, &rt, sp)?;
                            let res = Type::Int(res_w);
                            self.wrap_small(e, &res, sp);
                            return Ok(res);
                        }
                        if is_num(&lt) && is_num(&rt) && *op != Mod {
                            // int op float, or float op float: C usual arithmetic
                            // conversions apply; the result takes the float side's
                            // width (f64 wins when both are float)
                            let res = match (float_width(&lt), float_width(&rt)) {
                                (Some(w), _) if is_float_t(&lt) => Type::Float(w),
                                (_, Some(w)) => Type::Float(w),
                                _ => Type::Float(FloatW::F64),
                            };
                            return Ok(res);
                        }
                        err_at(
                            sp,
                            format!(
                                "operator '{:?}' cannot apply to {} and {}",
                                op,
                                lt.name(),
                                rt.name()
                            ),
                        )
                    }
                    BitAnd | BitOr | BitXor => {
                        if is_int(&lt) && is_int(&rt) {
                            let res_w = self.int_op_width(op, l, &lt, r, &rt, sp)?;
                            let res = Type::Int(res_w);
                            self.wrap_small(e, &res, sp);
                            return Ok(res);
                        }
                        err_at(
                            sp,
                            format!(
                                "bitwise '{:?}' needs int operands, got {} and {}",
                                op,
                                lt.name(),
                                rt.name()
                            ),
                        )
                    }
                    Shl | Shr => {
                        // shift amount may be any int width (C converts it);
                        // result takes the left operand's width
                        if is_int(&lt) && is_int(&rt) {
                            let res_w = int_width(&lt).unwrap();
                            let res = Type::Int(res_w);
                            self.wrap_small(e, &res, sp);
                            return Ok(res);
                        }
                        err_at(
                            sp,
                            format!(
                                "bitwise '{:?}' needs int operands, got {} and {}",
                                op,
                                lt.name(),
                                rt.name()
                            ),
                        )
                    }
                    Eq | Ne | Lt | Gt | Le | Ge => {
                        if lt == Type::Str && rt == Type::Str {
                            if matches!(op, Eq | Ne) {
                                let is_ne = *op == Ne;
                                let l_span = l.span;
                                let r_span = r.span;
                                let left = std::mem::replace(&mut **l, dummy_expr(l_span));
                                let right = std::mem::replace(&mut **r, dummy_expr(r_span));
                                let mut call = Spanned::new(
                                    ExprKind::Call("_wlel_streq".into(), vec![left, right]),
                                    sp,
                                );
                                if is_ne {
                                    let inner = call.clone();
                                    call = Spanned::new(
                                        ExprKind::Unary(UnOp::Not, Box::new(inner)),
                                        sp,
                                    );
                                }
                                *e = call;
                                return Ok(Type::Bool);
                            }
                            return err_at(
                                sp,
                                "relational '<, >, <=, >=' not supported on strings",
                            );
                        }
                        if is_int(&lt) && is_int(&rt) {
                            if lt != rt {
                                // same-width rule; an untyped literal adapts to
                                // the variable's width (range-checked in
                                // int_op_width), C usual conversions handle the rest
                                self.int_op_width(op, l, &lt, r, &rt, sp)?;
                                if !is_untyped_lit(l) && !is_untyped_lit(r) {
                                    return err_at(
                                        sp,
                                        format!(
                                            "comparison between {} and {}",
                                            lt.name(),
                                            rt.name()
                                        ),
                                    );
                                }
                            }
                        } else if lt != rt {
                            return err_at(
                                sp,
                                format!("comparison between {} and {}", lt.name(), rt.name()),
                            );
                        }
                        Ok(Type::Bool)
                    }
                    And | Or => {
                        if lt == Type::Bool && rt == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            err_at(sp, "'&&'/'||' need bool operands")
                        }
                    }
                }
            }
            ExprKind::Cast(ty, x) => {
                let from = self.expr_ty(x)?;
                let to = self.resolve_type_str(ty, sp)?;
                let num = |t: &Type| is_num(t);
                let ptr = |t: &Type| matches!(t, Type::Ptr(_));
                let ok = (num(&from) && num(&to))
                    || (ptr(&from) && ptr(&to))
                    || (ptr(&from) && is_int(&to))
                    || (is_int(&from) && ptr(&to))
                    || (from == Type::Str && ptr(&to))
                    || (ptr(&from) && to == Type::Str);
                if !ok {
                    return err_at(
                        sp,
                        format!("invalid cast from {} to {}", from.name(), to.name()),
                    );
                }
                Ok(to)
            }
            ExprKind::Index(base, idx) => {
                let bt = self.expr_ty(base)?;
                let it = self.expr_ty(idx)?;
                if !is_int(&it) {
                    return err_at(sp, format!("index must be int, got {}", it.name()));
                }
                match bt {
                    Type::Array(inner, _) => Ok(*inner),
                    Type::Ptr(inner) => Ok(*inner),
                    other => err_at(sp, format!("cannot index into {}", other.name())),
                }
            }
            ExprKind::Sizeof(ty) => {
                let t = self.resolve_type_str(ty, sp)?;
                if t == Type::Void {
                    return err_at(sp, "wlel_sizeof cannot take void");
                }
                Ok(Type::Int(IntW::I64))
            }
            ExprKind::New(ty, count) => {
                let inner = self.resolve_type_str(ty, sp)?;
                if inner == Type::Void {
                    return err_at(sp, "cannot allocate void via new()");
                }
                if matches!(inner, Type::Array(..)) {
                    return err_at(
                        sp,
                        format!(
                            "cannot new() an array type — use new(T, count) for '{}'",
                            ty
                        ),
                    );
                }
                if let Some(cnt) = count {
                    let ct = self.expr_ty(cnt)?;
                    if !is_int(&ct) {
                        return err_at(
                            sp,
                            format!("new(T, count): count must be int, got {}", ct.name()),
                        );
                    }
                }
                Ok(Type::Ptr(Box::new(inner)))
            }
            ExprKind::CurrentArena => {
                if self.arena_depth == 0 {
                    return err_at(
                        sp,
                        "wlel_arena() called outside of an active arena block",
                    );
                }
                Ok(Type::Ptr(Box::new(Type::Void)))
            }
            ExprKind::ArrayLit(elems) => {
                if elems.is_empty() {
                    return err_at(
                        sp,
                        "cannot infer type of empty array — use an explicit annotation",
                    );
                }
                let first = self.expr_ty(&mut elems[0])?;
                if first == Type::Void {
                    return err_at(sp, "array elements cannot be void");
                }
                for el in elems.iter_mut().skip(1) {
                    let t = self.expr_ty(el)?;
                    if t != first {
                        // untyped literals adapt to the array's element type
                        if is_untyped_lit(el)
                            && assignable_checked(&first, el, &t, sp).unwrap_or(false)
                        {
                            continue;
                        }
                        return err_at(
                            sp,
                            format!("mixed array element types: {} and {}", first.name(), t.name()),
                        );
                    }
                }
                Ok(Type::Array(Box::new(first), elems.len()))
            }
            ExprKind::Call(name, args) => {
                // sys module: process-level builtins, always available
                if let Some(rest) = name.strip_prefix("sys::") {
                    match rest {
                        "argc" => {
                            if !args.is_empty() {
                                return err_at(sp, "sys::argc() takes no arguments");
                            }
                            return Ok(Type::Int(IntW::I64));
                        }
                        "arg" => {
                            if args.len() != 1 {
                                return err_at(sp, "sys::arg(i) takes exactly 1 argument");
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            if !is_int(&t) {
                                return err_at(
                                    sp,
                                    format!("sys::arg(i): i must be int, got {}", t.name()),
                                );
                            }
                            return Ok(Type::Str);
                        }
                        "exit" => {
                            if args.len() != 1 {
                                return err_at(sp, "sys::exit(code) takes exactly 1 argument");
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            if !is_int(&t) {
                                return err_at(
                                    sp,
                                    format!("sys::exit(code): code must be int, got {}", t.name()),
                                );
                            }
                            return Ok(Type::Void);
                        }
                        other => {
                            return err_at(sp, format!("unknown sys function 'sys::{other}'"))
                        }
                    }
                }
                // std module
                if let Some(rest) = name.strip_prefix("std::") {
                    if !self.use_std {
                        return err_at(sp, format!("'{}' requires `use std;`", name));
                    }
                    self.std_called = true;
                    let want_args: &[Type] = match rest {
                        "print_int" | "println_int" => &[Type::Int(IntW::I64)],
                        "print_float" | "println_float" => &[Type::Float(FloatW::F64)],
                        "print_str" | "println_str" => &[Type::Str],
                        "strlen" => &[Type::Str],
                        "streq" => &[Type::Str, Type::Str],
                        "len" => &[],
                        "abs" => &[Type::Int(IntW::I64)],
                        "min" | "max" => &[Type::Int(IntW::I64), Type::Int(IntW::I64)],
                        // checked ops: result written through *int; false on overflow
                        "checked_add" | "checked_sub" | "checked_mul" => &[
                            Type::Int(IntW::I64),
                            Type::Int(IntW::I64),
                            Type::Ptr(Box::new(Type::Int(IntW::I64))),
                        ],
                        other => {
                            return err_at(sp, format!("unknown std function 'std::{other}'"))
                        }
                    };
                    let ret = match rest {
                        "streq" => Type::Bool,
                        "checked_add" | "checked_sub" | "checked_mul" => Type::Bool,
                        "strlen" => Type::Int(IntW::I64),
                        "abs" | "min" | "max" => Type::Int(IntW::I64),
                        "len" => return self.check_len(args, sp),
                        _ => Type::Void,
                    };
                    if args.len() != want_args.len() {
                        return err_at(
                            sp,
                            format!(
                                "std::{} takes {} argument(s), got {}",
                                rest,
                                want_args.len(),
                                args.len()
                            ),
                        );
                    }
                    for (i, a) in args.iter_mut().enumerate() {
                        let t = self.expr_ty(a)?;
                        if !assignable_checked(&want_args[i], a, &t, sp)? {
                            return err_at(
                                sp,
                                format!(
                                    "std::{} argument {}: expected {}, got {}",
                                    rest,
                                    i + 1,
                                    want_args[i].name(),
                                    t.name()
                                ),
                            );
                        }
                    }
                    return Ok(ret);
                }
                let cached: Option<FuncSig> = self.sigs.get(name).cloned();
                let sig = match cached {
                    Some(s) => {
                        self.called_funcs.insert(name.clone());
                        s
                    }
                    None => {
                        if name == "wlel_alloc" || name == "wlel_arena_new" {
                            if args.len() != 1 {
                                return err_at(sp, format!("{name} takes exactly 1 argument"));
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            if !is_int(&t) {
                                return err_at(
                                    sp,
                                    format!("{name} expects int, got {}", t.name()),
                                );
                            }
                            return Ok(Type::Ptr(Box::new(Type::Void)));
                        }
                        if name == "wlel_arena_alloc" {
                            if args.len() != 2 {
                                return err_at(
                                    sp,
                                    "wlel_arena_alloc takes exactly 2 arguments",
                                );
                            }
                            let a = self.expr_ty(&mut args[0])?;
                            let n = self.expr_ty(&mut args[1])?;
                            if !matches!(a, Type::Ptr(_)) || !is_int(&n) {
                                return err_at(
                                    sp,
                                    "wlel_arena_alloc(arena: *void, bytes: int)",
                                );
                            }
                            return Ok(Type::Ptr(Box::new(Type::Void)));
                        }
                        if name == "wlel_free" || name == "wlel_arena_free" {
                            if args.len() != 1 {
                                return err_at(sp, format!("{name} takes exactly 1 argument"));
                            }
                            let a = self.expr_ty(&mut args[0])?;
                            if !matches!(a, Type::Ptr(_)) {
                                return err_at(sp, format!("{name} expects a pointer"));
                            }
                            return Ok(Type::Void);
                        }
                        if name == "wlel_print_int" || name == "wlel_print_float" {
                            if args.len() != 1 {
                                return err_at(sp, format!("{name} takes exactly 1 argument"));
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            let want = if name.ends_with("_int") {
                                Type::Int(IntW::I64)
                            } else {
                                Type::Float(FloatW::F64)
                            };
                            if t != want {
                                return err_at(
                                    sp,
                                    format!("{name} expects {}, got {}", want.name(), t.name()),
                                );
                            }
                            return Ok(Type::Void);
                        }
                        if name == "wlel_print_str" {
                            if args.len() != 1 {
                                return err_at(sp, "wlel_print_str takes exactly 1 argument");
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            if t != Type::Str {
                                return err_at(
                                    sp,
                                    format!("wlel_print_str expects string, got {}", t.name()),
                                );
                            }
                            return Ok(Type::Void);
                        }
                        return err_at(sp, format!("call to undefined function '{name}'"));
                    }
                };
                if args.len() != sig.params.len() {
                    return err_at(
                        sp,
                        format!(
                            "'{}' takes {} argument(s), got {}",
                            name,
                            sig.params.len(),
                            args.len()
                        ),
                    );
                }
                for (i, a) in args.iter_mut().enumerate() {
                    let at = self.expr_ty(a)?;
                    let want = &sig.params[i];
                    // C-style array decay: [T; N] argument to *T parameter
                    let ok = match (want, &at) {
                        (Type::Ptr(w), Type::Array(el, _)) => **w == **el,
                        _ => assignable_checked(want, a, &at, sp)?,
                    };
                    if !ok {
                        return err_at(
                            sp,
                            format!(
                                "argument {} of '{}': expected {}, got {}",
                                i + 1,
                                name,
                                want.name(),
                                at.name()
                            ),
                        );
                    }
                }
                Ok(sig.ret)
            }
        }
    }

    fn check_block_mut(&mut self, b: &mut Block) -> CResult<()> {
        // a terminator inside this block does not terminate the enclosing one
        let outer_terminated = std::mem::replace(&mut self.block_terminated, false);
        self.scopes.push(HashMap::new());
        for s in b.0.iter_mut() {
            if self.block_terminated {
                self.warnings.push(CheckWarning {
                    msg: "unreachable statement".into(),
                    span: s.span,
                });
            }
            self.check_stmt(s)?;
        }
        self.pop_scope();
        self.block_terminated = outer_terminated;
        Ok(())
    }

    fn check_stmt(&mut self, s: &mut Stmt) -> CResult<()> {
        let sp = s.span;
        match &mut s.node {
            StmtKind::Let(name, ty_ann, init) => {
                let init_ty = self.expr_ty(init)?;
                let declared = match ty_ann.as_deref() {
                    Some(t) => Some(self.resolve_type_str(t, sp)?),
                    None => None,
                };
                if let Some(d) = declared {
                    if d == Type::Void {
                        return err_at(sp, format!("'{name}' cannot be void"));
                    }
                    // array literal may be shorter than the declared length
                    // (C zero-fills the rest); longer is an error
                    let array_ok = match (&d, &mut init.node) {
                        (Type::Array(dt, dn), ExprKind::ArrayLit(elems)) => {
                            if elems.len() > *dn {
                                return err_at(
                                    sp,
                                    format!(
                                        "array literal has {} element(s), '{}' holds {}",
                                        elems.len(),
                                        name,
                                        dn
                                    ),
                                );
                            }
                            for el in elems.iter_mut() {
                                let t = self.expr_ty(el)?;
                                if !assignable_checked(dt, el, &t, sp)? {
                                    return err_at(
                                        sp,
                                        format!(
                                            "array element: expected {}, got {}",
                                            dt.name(),
                                            t.name()
                                        ),
                                    );
                                }
                            }
                            true
                        }
                        _ => false,
                    };
                    if !array_ok && !assignable_checked(&d, init, &init_ty, sp)? {
                        return err_at(
                            sp,
                            format!(
                                "cannot initialize '{}' of type {} with {}",
                                name,
                                d.name(),
                                init_ty.name()
                            ),
                        );
                    }
                    self.declare("let", name, d, sp)?;
                } else {
                    if init_ty == Type::Void {
                        return err_at(
                            sp,
                            format!("cannot infer type of '{name}' from void expression"),
                        );
                    }
                    *ty_ann = Some(init_ty.name());
                    self.declare("let", name, init_ty, sp)?;
                }
                Ok(())
            }
            StmtKind::Assign(a) => {
                // a plain `x = v` writes x; it does not read it, so the read
                // flag the target check sets is restored afterwards
                let plain_write =
                    a.op == CompoundOp::Set && matches!(&a.target.node, ExprKind::Ident(_));
                let write_name = match &a.target.node {
                    ExprKind::Ident(n) if plain_write => Some(n.clone()),
                    _ => None,
                };
                let prev_read = write_name.as_deref().and_then(|n| self.is_read(n));
                let target_ty = self.expr_ty(&mut a.target)?;
                if let (Some(n), Some(prev)) = (&write_name, prev_read) {
                    self.mark_read(n, prev);
                }
                if matches!(target_ty, Type::Array(..)) {
                    return err_at(
                        sp,
                        "arrays are not assignable — assign elements or wrap the array in a struct",
                    );
                }
                let value_ty = self.expr_ty(&mut a.value)?;
                if !assignable_checked(&target_ty, &a.value, &value_ty, sp)? {
                    return err_at(
                        sp,
                        format!("cannot assign {} to {}", value_ty.name(), target_ty.name()),
                    );
                }
                if a.op != CompoundOp::Set {
                    if !is_num(&target_ty) {
                        return err_at(
                            sp,
                            format!(
                                "compound assignment requires a numeric target, got {}",
                                target_ty.name()
                            ),
                        );
                    }
                    if a.op == CompoundOp::Mod && !is_int(&target_ty) {
                        return err_at(sp, "%= requires int operands");
                    }
                }
                Ok(())
            }
            StmtKind::If(i) => self.check_if(i),
            StmtKind::While(cond, body) => {
                let c = self.expr_ty(cond)?;
                if c != Type::Bool {
                    return err_at(
                        sp,
                        format!("while condition must be bool, got {}", c.name()),
                    );
                }
                self.loop_depth += 1;
                let r = self.check_block_mut(body);
                self.loop_depth -= 1;
                r
            }
            StmtKind::For(var, start, end, body) => {
                let st = self.expr_ty(start)?;
                let et = self.expr_ty(end)?;
                if !is_int(&st) || !is_int(&et) {
                    return err_at(
                        sp,
                        format!(
                            "for loop range bounds must be int, got {} and {}",
                            st.name(),
                            et.name()
                        ),
                    );
                }
                self.loop_depth += 1;
                self.scopes.push(HashMap::new());
                let outer_terminated =
                    std::mem::replace(&mut self.block_terminated, false);
                self.declare("for", var, Type::Int(IntW::I64), sp)?;
                for s in body.0.iter_mut() {
                    if self.block_terminated {
                        self.warnings.push(CheckWarning {
                            msg: "unreachable statement".into(),
                            span: s.span,
                        });
                    }
                    self.check_stmt(s)?;
                }
                self.pop_scope();
                self.block_terminated = outer_terminated;
                self.loop_depth -= 1;
                Ok(())
            }
            StmtKind::Break => {
                if self.loop_depth == 0 {
                    return err_at(sp, "'break' outside of loop");
                }
                if self.defer_depth > 0 {
                    return err_at(sp, "'break' inside a defer block is not allowed");
                }
                self.block_terminated = true;
                Ok(())
            }
            StmtKind::Continue => {
                if self.loop_depth == 0 {
                    return err_at(sp, "'continue' outside of loop");
                }
                if self.defer_depth > 0 {
                    return err_at(sp, "'continue' inside a defer block is not allowed");
                }
                self.block_terminated = true;
                Ok(())
            }
            StmtKind::Return(e) => {
                if self.defer_depth > 0 {
                    return err_at(sp, "'return' inside a defer block is not allowed");
                }
                let actual = match e {
                    Some(e) => self.expr_ty(e)?,
                    None => Type::Void,
                };
                self.block_terminated = true;
                if self.current_ret == Type::Void {
                    if actual != Type::Void {
                        return err_at(sp, "void function cannot return a value");
                    }
                } else {
                    let ok = match e {
                        Some(expr) => {
                            assignable_checked(&self.current_ret, expr, &actual, sp)?
                        }
                        None => false,
                    };
                    if !ok {
                        return err_at(
                            sp,
                            format!(
                                "return type mismatch: expected {}, found {}",
                                self.current_ret.name(),
                                actual.name()
                            ),
                        );
                    }
                }
                Ok(())
            }
            StmtKind::ExprStmt(e) => {
                self.expr_ty(e)?;
                Ok(())
            }
            StmtKind::Block(b) => self.check_block_mut(b),
            StmtKind::Arena(cap, body) => {
                if let Some(c) = cap {
                    let ct = self.expr_ty(c)?;
                    if !is_int(&ct) {
                        return err_at(
                            sp,
                            format!("arena capacity must be int, got {}", ct.name()),
                        );
                    }
                }
                self.arena_depth += 1;
                let r = self.check_block_mut(body);
                self.arena_depth -= 1;
                r
            }
            StmtKind::Defer(DeferBody::Expr(e)) => {
                let t = self.expr_ty(e)?;
                if t != Type::Void {
                    return err_at(
                        sp,
                        format!("defer needs a void expression, got {}", t.name()),
                    );
                }
                Ok(())
            }
            StmtKind::Defer(DeferBody::Block(b)) => {
                self.defer_depth += 1;
                let r = self.check_block_mut(b);
                self.defer_depth -= 1;
                r
            }
        }
    }

    fn check_if(&mut self, i: &mut IfStmt) -> CResult<()> {
        let cond = self.expr_ty(&mut i.cond)?;
        if cond != Type::Bool {
            return err_at(
                i.cond.span,
                format!("if condition must be bool, got {}", cond.name()),
            );
        }
        self.check_block_mut(&mut i.then_body)?;
        match &mut i.else_branch {
            Some(ElseBranch::Block(b)) => self.check_block_mut(b)?,
            Some(ElseBranch::If(inner)) => self.check_if(inner)?,
            None => {}
        }
        Ok(())
    }
}

impl Checker {
    fn check_len(&mut self, args: &mut [Expr], span: Span) -> CResult<Type> {
        if args.len() != 1 {
            return err_at(span, "std::len takes exactly 1 argument");
        }
        let t = self.expr_ty(&mut args[0])?;
        match t {
            Type::Array(_, _) => Ok(Type::Int(IntW::I64)),
            _ => err_at(span, format!("std::len expects an array, got {}", t.name())),
        }
    }

    /// Width resolution for a binary int op: untyped literals adapt to the
    /// other operand's width (range-checked); two variables of different
    /// widths require an explicit cast.
    fn int_op_width(
        &mut self,
        op: &BinOp,
        l: &mut Expr,
        lt: &Type,
        r: &mut Expr,
        rt: &Type,
        sp: Span,
    ) -> CResult<IntW> {
        let lw = int_width(lt).unwrap();
        let rw = int_width(rt).unwrap();
        let lit_l = is_untyped_lit(l);
        let lit_r = is_untyped_lit(r);
        if lit_l && lit_r {
            return Ok(IntW::I64);
        }
        if lit_l {
            self.require_lit_fits(l, rw, sp)?;
            return Ok(rw);
        }
        if lit_r {
            self.require_lit_fits(r, lw, sp)?;
            return Ok(lw);
        }
        if lw == rw {
            return Ok(lw);
        }
        if matches!(op, BinOp::Shl | BinOp::Shr) {
            // shift amounts convert freely in C; the result is the lhs width
            return Ok(lw);
        }
        err_at(
            sp,
            format!(
                "mixed int widths: {} and {} — cast explicitly",
                lt.name(),
                rt.name()
            ),
        )
    }

    fn require_lit_fits(&mut self, e: &Expr, w: IntW, sp: Span) -> CResult<()> {
        if let Some(LitVal::Int(v, neg)) = lit_val(e) {
            if w.fits_literal(v, neg) {
                return Ok(());
            }
            let shown = if neg { -v } else { v };
            return err_at(
                sp,
                format!("literal {shown} does not fit in type {}", w.name()),
            );
        }
        Ok(())
    }

    /// Widths below 32 bits are promoted to `int` in C arithmetic, so wrap
    /// the node in an explicit cast to keep Wlel's wrap semantics exact.
    fn wrap_small(&mut self, e: &mut Expr, t: &Type, sp: Span) {
        let w = match int_width(t) {
            Some(w) if w.needs_wrap_cast() => w,
            _ => return,
        };
        let inner = std::mem::replace(e, dummy_expr(sp));
        *e = Spanned::new(ExprKind::Cast(w.name().into(), Box::new(inner)), sp);
    }
}

fn assignable(target: &Type, value: &Type) -> bool {
    target == value
        || (matches!(target, Type::Float(_)) && matches!(value, Type::Int(_)))
}

/// Assignability including untyped-literal adaptation: an unsuffixed literal
/// may take on the target's width (range-checked). Returns:
/// - Ok(true)  → assignable
/// - Ok(false) → not assignable
/// - Err(...)  → literal is assignable in principle but its value does not
///   fit the target width (clearer error than the generic one)
fn assignable_checked(target: &Type, value: &Expr, value_ty: &Type, span: Span) -> CResult<bool> {
    if assignable(target, value_ty) {
        return Ok(true);
    }
    if is_untyped_lit(value) {
        let lv = lit_val(value).unwrap();
        if let Some(w) = int_width(target) {
            if let LitVal::Int(v, neg) = lv {
                if w.fits_literal(v, neg) {
                    return Ok(true);
                }
                let shown = if neg { -v } else { v };
                return err_at(
                    span,
                    format!("literal {shown} does not fit in type {}", target.name()),
                );
            }
        }
        if let (Some(_), Some(LitVal::Float)) = (float_width(target), Some(lv)) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Files whose symbols are actually referenced: the file of every called
/// function plus the file of every used struct. Import paths are resolved
/// to canonical paths, so matching is exact.
fn called_files<'a>(
    program: &'a Program,
    called_funcs: &HashSet<String>,
    used_structs: &HashSet<String>,
) -> HashSet<&'a str> {
    let mut files: HashSet<&str> = HashSet::new();
    for f in &program.funcs {
        if called_funcs.contains(&f.name) {
            files.insert(f.file.as_str());
        }
    }
    for s in &program.structs {
        if used_structs.contains(&s.name) {
            files.insert(s.file.as_str());
        }
    }
    files
}

fn is_lvalue(e: &Expr) -> bool {
    matches!(
        &e.node,
        ExprKind::Ident(_) | ExprKind::Field(..) | ExprKind::Deref(_) | ExprKind::Index(..)
    )
}

/// placeholder used while rewriting string concat/equality nodes in place
fn dummy_expr(span: Span) -> Expr {
    Spanned::new(ExprKind::Bool(false), span)
}

/// True when executing this block guarantees the function returns.
fn guarantees_return(b: &Block) -> bool {
    match b.0.last().map(|s| &s.node) {
        Some(StmtKind::Return(_)) => true,
        Some(StmtKind::If(i)) => if_guarantees(i),
        Some(StmtKind::Block(inner)) => guarantees_return(inner),
        Some(StmtKind::Arena(_, inner)) => guarantees_return(inner),
        _ => false,
    }
}

fn if_guarantees(i: &IfStmt) -> bool {
    if !guarantees_return(&i.then_body) {
        return false;
    }
    match &i.else_branch {
        Some(ElseBranch::Block(b)) => guarantees_return(b),
        Some(ElseBranch::If(inner)) => if_guarantees(inner),
        None => false,
    }
}
