use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Void,
}

impl Type {
    fn from_name(name: &str) -> Option<Type> {
        match name {
            "int" => Some(Type::Int),
            "float" => Some(Type::Float),
            "bool" => Some(Type::Bool),
            "string" => Some(Type::Str),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Type::Int => "int",
            Type::Float => "float",
            Type::Bool => "bool",
            Type::Str => "string",
            Type::Void => "void",
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
    scopes: Vec<HashMap<String, Type>>,
    current_ret: Type,
}

impl Checker {
    /// Two passes: collect function signatures (forward refs allowed),
    /// then check every body.
    pub fn check(program: &Program) -> CResult<()> {
        let mut sigs = HashMap::new();
        for f in &program.funcs {
            let ret = match &f.ret_type {
                Some(t) => Self::resolve_type(t)?,
                None => Type::Void,
            };
            let mut params = Vec::with_capacity(f.params.len());
            for p in &f.params {
                let ty = match &p.ty {
                    Some(t) => Self::resolve_type(t)?,
                    None => return err(format!(
                        "function '{}': parameter '{}' needs a type annotation",
                        f.name, p.name
                    )),
                };
                params.push(ty);
            }
            if sigs.insert(f.name.clone(), FuncSig { params, ret }).is_some() {
                return err(format!("duplicate function '{}'", f.name));
            }
        }

        let mut cx = Checker { sigs: HashMap::new(), scopes: vec![HashMap::new()], current_ret: Type::Void };
        cx.sigs = sigs;
        for f in &program.funcs {
            let sig = cx.sigs.get(&f.name).unwrap().clone();
            let (sig_params, sig_ret) = (sig.params.clone(), sig.ret);
            cx.current_ret = sig_ret;
            cx.scopes = vec![HashMap::new()];
            for (p, t) in f.params.iter().zip(sig_params.iter()) {
                cx.declare(&f.name, &p.name, *t)?;
            }
            cx.check_block(&f.body)?;
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

    fn resolve_type(name: &str) -> CResult<Type> {
        Type::from_name(name)
            .ok_or_else(|| CheckError { msg: format!("unknown type '{name}'") })
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
                return Some(*t);
            }
        }
        None
    }

    fn check_block(&mut self, b: &Block) -> CResult<()> {
        self.scopes.push(HashMap::new());
        for s in &b.0 {
            self.check_stmt(s)?;
        }
        self.scopes.pop();
        Ok(())
    }

    fn check_stmt(&mut self, s: &Stmt) -> CResult<()> {
        match s {
            Stmt::Let(name, ty_ann, init) => {
                let init_ty = self.check_expr(init)?;
                let declared = match ty_ann {
                    Some(t) => Some(Self::resolve_type(t)?),
                    None => None,
                };
                if let Some(d) = declared {
                    if !assignable(d, init_ty) {
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
                    self.declare("let", name, init_ty)?;
                }
                Ok(())
            }
            Stmt::Assign(name, e) => {
                let var_ty = self
                    .lookup(name)
                    .ok_or_else(|| CheckError { msg: format!("undefined variable '{name}'") })?;
                let e_ty = self.check_expr(e)?;
                if !assignable(var_ty, e_ty) {
                    return err(format!(
                        "cannot assign {} to '{}' of type {}",
                        e_ty.name(),
                        name,
                        var_ty.name()
                    ));
                }
                Ok(())
            }
            Stmt::If(i) => {
                let cond = self.check_expr(&i.cond)?;
                if cond != Type::Bool {
                    return err(format!("if condition must be bool, got {}", cond.name()));
                }
                self.check_block(&i.then_body)?;
                match &i.else_branch {
                    Some(ElseBranch::Block(b)) => self.check_block(b)?,
                    Some(ElseBranch::If(inner)) => self.check_stmt(&Stmt::If((**inner).clone()))?,
                    None => {}
                }
                Ok(())
            }
            Stmt::While(cond, body) => {
                let c = self.check_expr(cond)?;
                if c != Type::Bool {
                    return err(format!("while condition must be bool, got {}", c.name()));
                }
                self.check_block(body)
            }
            Stmt::Return(e) => {
                let actual = match e {
                    Some(e) => self.check_expr(e)?,
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
                self.check_expr(e)?;
                Ok(())
            }
        }
    }

    fn check_expr(&mut self, e: &Expr) -> CResult<Type> {
        match e {
            Expr::Int(_) => Ok(Type::Int),
            Expr::Float(_) => Ok(Type::Float),
            Expr::Str(_) => Ok(Type::Str),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Ident(n) => self.lookup(n).ok_or_else(|| {
                CheckError { msg: format!("undefined variable '{n}'") }
            }),
            Expr::Unary(op, x) => {
                let t = self.check_expr(x)?;
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
            Expr::Binary(op, l, r) => {
                let lt = self.check_expr(l)?;
                let rt = self.check_expr(r)?;
                use BinOp::*;
                match op {
                    Add | Sub | Mul | Div | Mod => {
                        if lt == Type::Int && rt == Type::Int {
                            if *op == Mod {
                                return Ok(Type::Int);
                            }
                            return Ok(Type::Int);
                        }
                        if matches!(op, Add | Sub | Mul | Div)
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
            Expr::Call(name, args) => {
                let cached: Option<FuncSig> = self.sigs.get(name).map(|s| FuncSig {
                    params: s.params.clone(),
                    ret: s.ret,
                });
                let sig = match cached {
                    Some(s) => s,
                    None => {
                        // known builtin
                        if name == "wlel_print_int" {
                            if args.len() != 1 {
                                return err("wlel_print_int takes exactly 1 argument");
                            }
                            let t = self.check_expr(&args[0])?;
                            if t != Type::Int {
                                return err(format!("wlel_print_int expects int, got {}", t.name()));
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
                for (i, a) in args.iter().enumerate() {
                    let at = self.check_expr(a)?;
                    if at != sig.params[i] {
                        return err(format!(
                            "argument {} of '{}': expected {}, got {}",
                            i + 1,
                            name,
                            sig.params[i].name(),
                            at.name()
                        ));
                    }
                }
                Ok(sig.ret)
            }
        }
    }
}

fn assignable(target: Type, value: Type) -> bool {
    target == value || (target == Type::Float && value == Type::Int)
}

/// True when executing this block guarantees the function returns.
/// Rule (v0): the block's final statement is a `return`, or an `if`
/// whose branches ALL guarantee a return.
fn guarantees_return(b: &Block) -> bool {
    match b.0.last() {
        Some(Stmt::Return(_)) => true,
        Some(Stmt::If(i)) => if_guarantees(i),
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
