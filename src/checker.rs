use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
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
            "int" => Some(Type::Int),
            "float" => Some(Type::Float),
            "bool" => Some(Type::Bool),
            "string" => Some(Type::Str),
            "void" => Some(Type::Void),
            _ => None,
        }
    }

    fn name(&self) -> String {
        match self {
            Type::Int => "int".into(),
            Type::Float => "float".into(),
            Type::Bool => "bool".into(),
            Type::Str => "string".into(),
            Type::Void => "void".into(),
            Type::Struct(n) => n.clone(),
            Type::Ptr(t) => format!("*{}", t.name()),
            Type::Array(t, n) => format!("[{}; {}]", t.name(), n),
        }
    }
}

#[derive(Clone)]
struct FuncSig {
    params: Vec<Type>,
    ret: Type,
}

#[derive(Debug)]
pub struct CheckError {
    pub msg: String,
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for CheckError {}

type CResult<T> = Result<T, CheckError>;

fn err<T>(msg: impl Into<String>) -> CResult<T> {
    Err(CheckError { msg: msg.into() })
}

pub struct Checker {
    sigs: HashMap<String, FuncSig>,
    structs: HashMap<String, Vec<(String, Type)>>,
    scopes: Vec<HashMap<String, Type>>,
    current_ret: Type,
    use_std: bool,
}

impl Checker {
    /// Passes: (0) structs, (1) function signatures, (2) bodies.
    /// Inferred `let` annotations are written back into the AST so the
    /// codegen can emit exact C types. Auto-deref on field access is
    /// rewritten into the AST here as well.
    pub fn check(program: &mut Program) -> CResult<()> {
        let use_std = program.uses.iter().any(|u| u.path.is_none());
        let mut cx = Checker {
            sigs: HashMap::new(),
            structs: HashMap::new(),
            scopes: vec![HashMap::new()],
            current_ret: Type::Void,
            use_std,
        };

        // pass 0: register struct names, then resolve field types
        for st in &program.structs {
            if cx.structs.insert(st.name.clone(), Vec::new()).is_some() {
                return err(format!("duplicate struct '{}'", st.name));
            }
        }
        for st in &program.structs {
            let mut fields = Vec::with_capacity(st.fields.len());
            for (fname, fty) in &st.fields {
                let t = cx.resolve_type_str(fty)?;
                if t == Type::Void {
                    return err(format!(
                        "struct '{}': field '{}' cannot be void",
                        st.name, fname
                    ));
                }
                fields.push((fname.clone(), t));
            }
            cx.structs.insert(st.name.clone(), fields);
        }

        // pass 1: signatures
        for f in &program.funcs {
            let ret = cx.resolve_type_str(f.ret_type.as_deref().unwrap_or("void"))?;
            if matches!(ret, Type::Array(..)) {
                return err(format!(
                    "function '{}': cannot return an array — return a pointer or wrap it in a struct",
                    f.name
                ));
            }
            let mut params = Vec::with_capacity(f.params.len());
            for p in &f.params {
                let t = cx.resolve_type_str(p.ty.as_deref().unwrap_or("int"))?;
                if t == Type::Void {
                    return err(format!(
                        "function '{}': parameter '{}' cannot be void",
                        f.name, p.name
                    ));
                }
                if matches!(t, Type::Array(..)) {
                    return err(format!(
                        "function '{}': parameter '{}' — pass arrays by pointer or wrap them in a struct",
                        f.name, p.name
                    ));
                }
                params.push(t);
            }
            if cx.sigs.insert(f.name.clone(), FuncSig { params, ret }).is_some() {
                return err(format!("duplicate function '{}'", f.name));
            }
        }

        // pass 2: bodies
        for f in program.funcs.iter_mut() {
            let sig = cx.sigs.get(&f.name).unwrap().clone();
            let sig_params = sig.params.clone();
            let sig_ret = sig.ret.clone();
            cx.current_ret = sig_ret.clone();
            cx.scopes = vec![HashMap::new()];
            for (p, t) in f.params.iter().zip(sig_params.iter()) {
                cx.declare(&f.name, &p.name, t.clone())?;
            }
            cx.check_block_mut(&mut f.body)?;
            if sig_ret != Type::Void && !guarantees_return(&f.body) {
                return err(format!(
                    "function '{}' declares '-> {}' but has no return statement",
                    f.name,
                    sig_ret.name()
                ));
            }
        }
        Ok(())
    }

    fn resolve_type_str(&self, s: &str) -> CResult<Type> {
        // array form: [T; N]
        if let Some(rest) = s.strip_prefix('[') {
            let close = rest
                .rfind(']')
                .ok_or_else(|| CheckError { msg: format!("bad array type '{s}'") })?;
            let inner_s = &rest[..close];
            let n: usize = inner_s
                .rsplit(';')
                .next()
                .unwrap_or("")
                .trim()
                .parse()
                .map_err(|_| CheckError { msg: format!("bad array length in '{s}'") })?;
            let inner = self.resolve_type_str(inner_s[..inner_s.len() - inner_s.rsplit(';').next().unwrap().len()].trim_end_matches(';'))?;
            if n == 0 {
                return err(format!("array length must be > 0 in '{s}'"));
            }
            return Ok(Type::Array(Box::new(inner), n));
        }
        let stars = s.chars().take_while(|c| *c == '*').count();
        let base = &s[stars..];
        let t = match Type::from_builtin(base) {
            Some(t) => t,
            None => {
                if self.structs.contains_key(base) {
                    Type::Struct(base.to_string())
                } else {
                    return err(format!("unknown type '{}'", base));
                }
            }
        };
        let mut out = t;
        for _ in 0..stars {
            out = Type::Ptr(Box::new(out));
        }
        Ok(out)
    }

    fn declare(&mut self, ctx: &str, name: &str, ty: Type) -> CResult<()> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.insert(name.to_string(), ty).is_some() {
            return err(format!("{ctx}: variable '{name}' already declared in this scope"));
        }
        Ok(())
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    fn struct_field(&self, sname: &str, field: &str) -> CResult<Type> {
        let fields = self
            .structs
            .get(sname)
            .ok_or_else(|| CheckError { msg: format!("unknown struct '{sname}'") })?;
        fields
            .iter()
            .find(|(n, _)| n == field)
            .map(|(_, t)| t.clone())
            .ok_or_else(|| CheckError {
                msg: format!("struct '{sname}' has no field '{field}'"),
            })
    }

    /// Types an expression, mutating auto-deref into Field objects.
    fn expr_ty(&mut self, e: &mut Expr) -> CResult<Type> {
        match e {
            Expr::Int(_) => Ok(Type::Int),
            Expr::Float(_) => Ok(Type::Float),
            Expr::Str(_) => Ok(Type::Str),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Ident(n) => self
                .lookup(n)
                .ok_or_else(|| CheckError { msg: format!("undefined variable '{n}'") }),
            Expr::Unary(op, x) => {
                let t = self.expr_ty(x)?;
                match op {
                    UnOp::Neg => match t {
                        Type::Int | Type::Float => Ok(t),
                        _ => err(format!("unary '-' needs a number, got {}", t.name())),
                    },
                    UnOp::Not => {
                        if t == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            err(format!("'!' needs bool, got {}", t.name()))
                        }
                    }
                }
            }
            Expr::AddrOf(x) => {
                let t = self.expr_ty(x)?;
                if !is_lvalue(x) {
                    return err(" '&' needs a variable, field or dereference");
                }
                Ok(Type::Ptr(Box::new(t)))
            }
            Expr::Deref(x) => {
                let t = self.expr_ty(x)?;
                match t {
                    Type::Ptr(inner) => Ok(*inner),
                    other => err(format!("cannot dereference non-pointer {}", other.name())),
                }
            }
            Expr::Field(obj, field) => {
                let ot = self.expr_ty(obj)?;
                // auto-deref: p.x where p: *Point becomes (*p).x
                let sname = match ot {
                    Type::Struct(n) => n,
                    Type::Ptr(inner) => match *inner {
                        Type::Struct(n) => {
                            **obj = Expr::Deref(Box::new((**obj).clone()));
                            n
                        }
                        other => {
                            return err(format!(
                                "field '{field}' on pointer to non-struct {}",
                                other.name()
                            ))
                        }
                    },
                    other => {
                        return err(format!(
                            "field access '{field}' on non-struct {}",
                            other.name()
                        ))
                    }
                };
                self.struct_field(&sname, field)
            }
            Expr::StructLit(name, fields) => {
                let declared: Vec<(String, Type)> = match self.structs.get(name) {
                    Some(f) => f.clone(),
                    None => return err(format!("unknown struct '{name}'")),
                };
                if fields.len() != declared.len() {
                    return err(format!(
                        "struct '{}': expected {} field(s), got {}",
                        name,
                        declared.len(),
                        fields.len()
                    ));
                }
                for (i, (fname, fexpr)) in fields.iter_mut().enumerate() {
                    if *fname != declared[i].0 {
                        return err(format!(
                            "struct '{}': field {} should be '{}', found '{}'",
                            name,
                            i + 1,
                            declared[i].0,
                            fname
                        ));
                    }
                    let want = declared[i].1.clone();
                    let got = self.expr_ty(fexpr)?;
                    if !assignable(&want, &got) {
                        return err(format!(
                            "struct '{}': field '{}': expected {}, got {}",
                            name,
                            fname,
                            want.name(),
                            got.name()
                        ));
                    }
                }
                Ok(Type::Struct(name.clone()))
            }
            Expr::Binary(op, l, r) => {
                let lt = self.expr_ty(l)?;
                let rt = self.expr_ty(r)?;
                use BinOp::*;
                match op {
                    Add | Sub | Mul | Div | Mod => {
                        if lt == Type::Int && rt == Type::Int {
                            return Ok(Type::Int);
                        }
                        if *op != Mod
                            && matches!(lt, Type::Int | Type::Float)
                            && matches!(rt, Type::Int | Type::Float)
                        {
                            return Ok(Type::Float);
                        }
                        err(format!(
                            "operator '{:?}' cannot apply to {} and {}",
                            op,
                            lt.name(),
                            rt.name()
                        ))
                    }
                    Eq | Ne | Lt | Gt | Le | Ge => {
                        if lt == Type::Str || rt == Type::Str {
                            return err("string comparison is not supported yet (planned: wlel_streq)");
                        }
                        if lt != rt {
                            return err(format!(
                                "comparison between {} and {}",
                                lt.name(),
                                rt.name()
                            ));
                        }
                        Ok(Type::Bool)
                    }
                    And | Or => {
                        if lt == Type::Bool && rt == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            err("'&&'/'||' need bool operands")
                        }
                    }
                }
            }
            Expr::Cast(ty, x) => {
                let from = self.expr_ty(x)?;
                let to = self.resolve_type_str(ty)?;
                let num = |t: &Type| matches!(t, Type::Int | Type::Float);
                let ptr = |t: &Type| matches!(t, Type::Ptr(_));
                let ok = (num(&from) && num(&to))
                    || (ptr(&from) && ptr(&to))
                    || (ptr(&from) && to == Type::Int)
                    || (from == Type::Int && ptr(&to))
                    || (from == Type::Str && ptr(&to))
                    || (ptr(&from) && to == Type::Str);
                if !ok {
                    return err(format!("invalid cast from {} to {}", from.name(), to.name()));
                }
                Ok(to)
            }
            Expr::Index(base, idx) => {
                let bt = self.expr_ty(base)?;
                let it = self.expr_ty(idx)?;
                if it != Type::Int {
                    return err(format!("index must be int, got {}", it.name()));
                }
                match bt {
                    Type::Array(inner, _) => Ok(*inner),
                    Type::Ptr(inner) => Ok(*inner),
                    other => err(format!("cannot index into {}", other.name())),
                }
            }
            Expr::Sizeof(ty) => {
                let t = self.resolve_type_str(ty)?;
                if t == Type::Void {
                    return err("wlel_sizeof cannot take void");
                }
                Ok(Type::Int)
            }
            Expr::ArrayLit(elems) => {
                if elems.is_empty() {
                    return err("cannot infer type of empty array — use an explicit annotation");
                }
                let first = self.expr_ty(&mut elems[0])?;
                if first == Type::Void {
                    return err("array elements cannot be void");
                }
                for el in elems.iter_mut().skip(1) {
                    let t = self.expr_ty(el)?;
                    if t != first {
                        return err(format!(
                            "mixed array element types: {} and {}",
                            first.name(),
                            t.name()
                        ));
                    }
                }
                Ok(Type::Array(Box::new(first), elems.len()))
            }
            Expr::Call(name, args) => {
                // std module
                if let Some(rest) = name.strip_prefix("std::") {
                    if !self.use_std {
                        return err(format!("'{}' requires `use std;`", name));
                    }
                    let want_args: &[Type] = match rest {
                        "print_int" | "println_int" => &[Type::Int],
                        "print_float" | "println_float" => &[Type::Float],
                        "print_str" | "println_str" => &[Type::Str],
                        "streq" => &[Type::Str, Type::Str],
                        "len" => &[],
                        "abs" => &[Type::Int],
                        "min" | "max" => &[Type::Int, Type::Int],
                        other => return err(format!("unknown std function 'std::{other}'")),
                    };
                    let ret = match rest {
                        "streq" => Type::Bool,
                        "abs" | "min" | "max" => Type::Int,
                        "len" => return self.check_len(args),
                        _ => Type::Void,
                    };
                    if args.len() != want_args.len() {
                        return err(format!(
                            "std::{} takes {} argument(s), got {}",
                            rest,
                            want_args.len(),
                            args.len()
                        ));
                    }
                    for (i, a) in args.iter_mut().enumerate() {
                        let t = self.expr_ty(a)?;
                        if t != want_args[i] {
                            return err(format!(
                                "std::{} argument {}: expected {}, got {}",
                                rest,
                                i + 1,
                                want_args[i].name(),
                                t.name()
                            ));
                        }
                    }
                    return Ok(ret);
                }
                let cached: Option<FuncSig> = self.sigs.get(name).cloned();
                let sig = match cached {
                    Some(s) => s,
                    None => {
                        if name == "wlel_alloc" || name == "wlel_arena_new" {
                            if args.len() != 1 {
                                return err(format!("{name} takes exactly 1 argument"));
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            if t != Type::Int {
                                return err(format!("{name} expects int, got {}", t.name()));
                            }
                            return Ok(Type::Ptr(Box::new(Type::Void)));
                        }
                        if name == "wlel_arena_alloc" {
                            if args.len() != 2 {
                                return err("wlel_arena_alloc takes exactly 2 arguments");
                            }
                            let a = self.expr_ty(&mut args[0])?;
                            let n = self.expr_ty(&mut args[1])?;
                            if !matches!(a, Type::Ptr(_)) || n != Type::Int {
                                return err("wlel_arena_alloc(arena: *void, bytes: int)");
                            }
                            return Ok(Type::Ptr(Box::new(Type::Void)));
                        }
                        if name == "wlel_free" || name == "wlel_arena_free" {
                            if args.len() != 1 {
                                return err(format!("{name} takes exactly 1 argument"));
                            }
                            let a = self.expr_ty(&mut args[0])?;
                            if !matches!(a, Type::Ptr(_)) {
                                return err(format!("{name} expects a pointer"));
                            }
                            return Ok(Type::Void);
                        }
                        if name == "wlel_print_int" || name == "wlel_print_float" {
                            if args.len() != 1 {
                                return err(format!("{name} takes exactly 1 argument"));
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            let want = if name.ends_with("_int") { Type::Int } else { Type::Float };
                            if t != want {
                                return err(format!("{name} expects {}, got {}", want.name(), t.name()));
                            }
                            return Ok(Type::Void);
                        }
                        if name == "wlel_print_str" {
                            if args.len() != 1 {
                                return err("wlel_print_str takes exactly 1 argument");
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            if t != Type::Str {
                                return err(format!("wlel_print_str expects string, got {}", t.name()));
                            }
                            return Ok(Type::Void);
                        }
                        return err(format!("call to undefined function '{name}'"));
                    }
                };
                if args.len() != sig.params.len() {
                    return err(format!(
                        "'{}' takes {} argument(s), got {}",
                        name,
                        sig.params.len(),
                        args.len()
                    ));
                }
                for (i, a) in args.iter_mut().enumerate() {
                    let at = self.expr_ty(a)?;
                    let want = &sig.params[i];
                    // C-style array decay: [T; N] argument to *T parameter
                    let ok = match (want, &at) {
                        (Type::Ptr(w), Type::Array(el, _)) => **w == **el,
                        _ => want == &at,
                    };
                    if !ok {
                        return err(format!(
                            "argument {} of '{}': expected {}, got {}",
                            i + 1,
                            name,
                            want.name(),
                            at.name()
                        ));
                    }
                }
                Ok(sig.ret)
            }
        }
    }

    fn check_block_mut(&mut self, b: &mut Block) -> CResult<()> {
        self.scopes.push(HashMap::new());
        for s in b.0.iter_mut() {
            self.check_stmt(s)?;
        }
        self.scopes.pop();
        Ok(())
    }

    fn check_stmt(&mut self, s: &mut Stmt) -> CResult<()> {
        match s {
            Stmt::Let(name, ty_ann, init) => {
                let init_ty = self.expr_ty(init)?;
                let declared = match ty_ann.as_deref() {
                    Some(t) => Some(self.resolve_type_str(t)?),
                    None => None,
                };
                if let Some(d) = declared {
                    if d == Type::Void {
                        return err(format!("'{name}' cannot be void"));
                    }
                    // array literal may be shorter than the declared length
                    // (C zero-fills the rest); longer is an error
                    let array_ok = match (&d, init) {
                        (Type::Array(dt, dn), Expr::ArrayLit(elems)) => {
                            if elems.len() > *dn {
                                return err(format!(
                                    "array literal has {} element(s), '{}' holds {}",
                                    elems.len(),
                                    name,
                                    dn
                                ));
                            }
                            for el in elems {
                                let t = self.expr_ty(el)?;
                                if !assignable(dt, &t) {
                                    return err(format!(
                                        "array element: expected {}, got {}",
                                        dt.name(),
                                        t.name()
                                    ));
                                }
                            }
                            true
                        }
                        _ => false,
                    };
                    if !array_ok && !assignable(&d, &init_ty) {
                        return err(format!(
                            "cannot initialize '{}' of type {} with {}",
                            name,
                            d.name(),
                            init_ty.name()
                        ));
                    }
                    self.declare("let", name, d)?;
                } else {
                    if init_ty == Type::Void {
                        return err(format!("cannot infer type of '{name}' from void expression"));
                    }
                    *ty_ann = Some(init_ty.name());
                    self.declare("let", name, init_ty)?;
                }
                Ok(())
            }
            Stmt::Assign(a) => {
                let target_ty = self.expr_ty(&mut a.target)?;
                if matches!(target_ty, Type::Array(..)) {
                    return err("arrays are not assignable — assign elements or wrap the array in a struct");
                }
                let value_ty = self.expr_ty(&mut a.value)?;
                if !assignable(&target_ty, &value_ty) {
                    return err(format!(
                        "cannot assign {} to {}",
                        value_ty.name(),
                        target_ty.name()
                    ));
                }
                Ok(())
            }
            Stmt::If(i) => {
                let cond = self.expr_ty(&mut i.cond)?;
                if cond != Type::Bool {
                    return err(format!("if condition must be bool, got {}", cond.name()));
                }
                self.check_block_mut(&mut i.then_body)?;
                match &mut i.else_branch {
                    Some(ElseBranch::Block(b)) => self.check_block_mut(b)?,
                    Some(ElseBranch::If(inner)) => {
                        self.check_stmt(&mut Stmt::If((**inner).clone()))?
                    }
                    None => {}
                }
                Ok(())
            }
            Stmt::While(cond, body) => {
                let c = self.expr_ty(cond)?;
                if c != Type::Bool {
                    return err(format!("while condition must be bool, got {}", c.name()));
                }
                self.check_block_mut(body)
            }
            Stmt::Return(e) => {
                let actual = match e {
                    Some(e) => self.expr_ty(e)?,
                    None => Type::Void,
                };
                if self.current_ret == Type::Void {
                    if actual != Type::Void {
                        return err("void function cannot return a value");
                    }
                } else if actual != self.current_ret {
                    return err(format!(
                        "return type mismatch: expected {}, found {}",
                        self.current_ret.name(),
                        actual.name()
                    ));
                }
                Ok(())
            }
            Stmt::ExprStmt(e) => {
                self.expr_ty(e)?;
                Ok(())
            }
            Stmt::Block(b) => self.check_block_mut(b),
            Stmt::Defer(e) => {
                let t = self.expr_ty(e)?;
                if t != Type::Void {
                    return err(format!("defer needs a void expression, got {}", t.name()));
                }
                Ok(())
            }
        }
    }
}

impl Checker {
    fn check_len(&mut self, args: &mut [Expr]) -> CResult<Type> {
        if args.len() != 1 {
            return err("std::len takes exactly 1 argument");
        }
        let t = self.expr_ty(&mut args[0])?;
        match t {
            Type::Array(_, _) => Ok(Type::Int),
            _ => err(format!("std::len expects an array, got {}", t.name())),
        }
    }
}

fn assignable(target: &Type, value: &Type) -> bool {
    target == value || (*target == Type::Float && *value == Type::Int)
}

fn is_lvalue(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Ident(_) | Expr::Field(..) | Expr::Deref(_) | Expr::Index(..)
    )
}

/// True when executing this block guarantees the function returns.
fn guarantees_return(b: &Block) -> bool {
    match b.0.last() {
        Some(Stmt::Return(_)) => true,
        Some(Stmt::If(i)) => if_guarantees(i),
        Some(Stmt::Block(inner)) => guarantees_return(inner),
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
