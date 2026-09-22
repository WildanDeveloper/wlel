//! Experimental QBE backend (Fase 3 Item 8).
//!
//! Transpiles a *subset* of Wlel to QBE IL (`--backend qbe`). The subset is
//! the arena-free core of the language: integer widths, floats, bools,
//! pointers, stack arrays, control flow, functions and `extern fn`. Every
//! construct outside the subset is refused with a `file:line:col` message
//! instead of silently miscompiling.
//!
//! Design notes:
//! - QBE repairs non-SSA input automatically (see the QBE IL manual, "Phi"),
//!   so variables live in plain redefinable temporaries — no phi plumbing.
//!   Only address-taken locals and arrays get `alloc8` stack slots.
//! - Narrow integers are kept canonical in registers: u8/u16 zero-extended,
//!   i8/i16 sign-extended, everything ≤32-bit in `w`, 64-bit in `l`. Every
//!   arithmetic/bitwise/shift result on a <32-bit type is re-canonicalized
//!   with `ext*`, which implements Wlel's automatic wrap-cast for sub-32-bit
//!   arithmetic exactly like the C backend's implicit casts.
//! - All libc contact goes through a tiny runtime translation unit
//!   (`RUNTIME_C`) so the same IL links on POSIX and Windows.

use crate::ast::*;
use crate::span::Span;

// ---------------------------------------------------------------------------
// types

/// the QBE-facing view of a checker-annotated type name
#[derive(Clone, Debug, PartialEq)]
pub enum QT {
    Int { bits: u8, signed: bool },
    Float { bits: u8 },
    Bool,
    Str,
    Ptr,
    Array(Box<QT>, usize),
    Void,
}

impl QT {
    /// parse a canonical checker type name ("int", "u8", "*Pt", "[int; 3]")
    fn parse(name: &str) -> Option<QT> {
        let stars = name.chars().take_while(|c| *c == '*').count();
        if stars > 0 {
            return QT::parse(&name[stars..]).map(|_| QT::Ptr);
        }
        if let Some(inner) = name.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let (elem, n) = inner.rsplit_once("; ")?;
            let n: usize = n.trim().parse().ok()?;
            return Some(QT::Array(Box::new(QT::parse(elem.trim())?), n));
        }
        Some(match name {
            "int" | "i64" => QT::Int { bits: 64, signed: true },
            "i32" => QT::Int { bits: 32, signed: true },
            "i16" => QT::Int { bits: 16, signed: true },
            "i8" => QT::Int { bits: 8, signed: true },
            "u64" => QT::Int { bits: 64, signed: false },
            "u32" => QT::Int { bits: 32, signed: false },
            "u16" => QT::Int { bits: 16, signed: false },
            "u8" | "byte" | "char" => QT::Int { bits: 8, signed: false },
            "usize" => QT::Int { bits: 64, signed: false },
            "float" | "f64" => QT::Float { bits: 64 },
            "f32" => QT::Float { bits: 32 },
            "bool" => QT::Bool,
            "string" => QT::Str,
            "void" => QT::Void,
            _ => return None,
        })
    }

    /// QBE base type of a *value* of this type (None: not a first-class value)
    fn base(&self) -> Option<&'static str> {
        match self {
            QT::Int { bits, .. } => Some(if *bits == 64 { "l" } else { "w" }),
            QT::Float { bits } => Some(if *bits == 64 { "d" } else { "s" }),
            QT::Bool => Some("w"),
            QT::Str | QT::Ptr => Some("l"),
            QT::Array(..) | QT::Void => None,
        }
    }

    /// size in memory (bytes), C layout
    fn size(&self) -> usize {
        match self {
            QT::Int { bits, .. } | QT::Float { bits } => (*bits as usize) / 8,
            QT::Bool => 1,
            QT::Str | QT::Ptr => 8,
            QT::Array(e, n) => e.size() * n,
            QT::Void => 1,
        }
    }

    /// ABI type letter for params/returns (sub-word forms below 32 bits)
    fn abi(&self) -> Option<String> {
        match self {
            QT::Int { bits: 8, signed: true } => Some("sb".into()),
            QT::Int { bits: 8, signed: false } => Some("ub".into()),
            QT::Int { bits: 16, signed: true } => Some("sh".into()),
            QT::Int { bits: 16, signed: false } => Some("uh".into()),
            QT::Bool => Some("ub".into()),
            other => other.base().map(str::to_string),
        }
    }

    fn narrow_bits(&self) -> Option<(u8, bool)> {
        match self {
            QT::Int { bits, signed } if *bits < 32 => Some((*bits, *signed)),
            _ => None,
        }
    }
}

/// memory load instruction for a value of this type at an address
fn load_op(t: &QT) -> Option<&'static str> {
    Some(match t {
        QT::Int { bits: 8, signed: true } => "loadsb",
        QT::Int { bits: 8, signed: false } | QT::Bool => "loadub",
        QT::Int { bits: 16, signed: true } => "loadsh",
        QT::Int { bits: 16, signed: false } => "loaduh",
        QT::Int { bits: 32, signed: true } => "loadsw",
        QT::Int { bits: 32, signed: false } => "loaduw",
        QT::Int { bits: 64, .. } => "loadl",
        QT::Float { bits: 32 } => "loads",
        QT::Float { bits: 64 } => "loadd",
        QT::Str | QT::Ptr => "loadl",
        _ => return None,
    })
}

fn store_op(t: &QT) -> Option<&'static str> {
    Some(match t {
        QT::Int { bits: 8, .. } | QT::Bool => "storeb",
        QT::Int { bits: 16, .. } => "storeh",
        QT::Int { bits: 32, .. } => "storew",
        QT::Int { bits: 64, .. } => "storel",
        QT::Float { bits: 32 } => "stores",
        QT::Float { bits: 64 } => "stored",
        QT::Str | QT::Ptr => "storel",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// generator state

struct QErr {
    file: String,
    span: Span,
    msg: String,
}

/// how a name is stored: a redefinable register temporary (QBE repairs SSA)
/// or an `alloc8` stack slot (address-taken variables and arrays)
#[derive(Clone)]
enum Binding {
    Tmp(String),
    Slot { addr: String, ty: QT },
}

struct Qb {
    out: String,
    /// per-function buffer: everything after `@start`; assembled with the
    /// hoisted allocations into `out` when the function ends
    fn_out: String,
    /// stack slots discovered while generating the body; QBE only accepts
    /// `alloc` in the entry block, so they are hoisted into `@start`
    allocs: Vec<(String, usize)>,
    data: Vec<String>,
    strs: Vec<(String, String)>, // literal text -> data symbol
    tmp: usize,
    lbl: usize,
    scopes: Vec<std::collections::HashMap<String, Binding>>,
    addr_taken: std::collections::HashSet<String>,
    breaks: Vec<String>,
    conts: Vec<String>,
    cur_ret: QT,
    term: bool,
    err: Option<QErr>,
    cur_file: String,
    fns: std::collections::HashMap<String, (Vec<QT>, QT)>, // callee signatures
}

impl Qb {
    fn new() -> Qb {
        Qb {
            out: String::new(),
            fn_out: String::new(),
            allocs: Vec::new(),
            data: Vec::new(),
            strs: Vec::new(),
            tmp: 0,
            lbl: 0,
            scopes: vec![Default::default()],
            addr_taken: Default::default(),
            breaks: Vec::new(),
            conts: Vec::new(),
            cur_ret: QT::Void,
            term: false,
            err: None,
            cur_file: String::new(),
            fns: Default::default(),
        }
    }

    fn fail(&mut self, span: Span, msg: impl Into<String>) {
        if self.err.is_none() {
            self.err = Some(QErr { file: self.cur_file.clone(), span, msg: msg.into() });
        }
    }

    fn tmp(&mut self) -> String {
        self.tmp += 1;
        format!("%t{}", self.tmp)
    }

    fn lbl(&mut self, tag: &str) -> String {
        self.lbl += 1;
        format!("@{}_{}", tag, self.lbl)
    }

    /// emit an instruction into the current (open, unterminated) block
    fn emit(&mut self, line: impl AsRef<str>) {
        debug_assert!(!self.term, "instruction after terminator");
        self.fn_out.push('\t');
        self.fn_out.push_str(line.as_ref());
        self.fn_out.push('\n');
    }

    /// emit a block terminator (jmp/jnz/ret/hlt)
    fn term_line(&mut self, line: impl AsRef<str>) {
        self.emit(line);
        self.term = true;
    }

    /// start a block. When the previous block was left unterminated the
    /// parser inserts the fallthrough jump automatically; when it was left
    /// terminated (e.g. by `ret`) this opens fresh — usually dead — code.
    fn label(&mut self, l: &str) {
        self.fn_out.push_str(l);
        self.fn_out.push('\n');
        self.term = false;
    }

    /// reserve a stack slot; the alloc instruction itself is hoisted into
    /// the function's entry block (QBE requirement)
    fn alloc(&mut self, bytes: usize) -> String {
        let t = self.tmp();
        self.allocs.push((t.clone(), bytes));
        t
    }

    /// fresh label + start it
    fn here(&mut self, tag: &str) -> String {
        let l = self.lbl(tag);
        self.label(&l);
        l
    }

    // -- scopes ---------------------------------------------------------

    fn bind(&mut self, name: &str, b: Binding) {
        self.scopes.last_mut().unwrap().insert(name.to_string(), b);
    }

    fn lookup(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name).cloned())
    }

    // -- values ---------------------------------------------------------

    /// canonicalize an arithmetic result of a <32-bit integer type
    /// (Wlel: automatic wrap-cast for sub-32-bit arithmetic)
    fn narrow_fixup(&mut self, v: &str, ty: &QT) -> String {
        match ty.narrow_bits() {
            None => v.to_string(),
            Some((bits, signed)) => {
                let r = self.tmp();
                let insn = match (bits, signed) {
                    (8, true) => "extsb",
                    (8, false) => "extub",
                    (16, true) => "extsh",
                    _ => "extuh",
                };
                self.emit(format!("{r} =w {insn} {v}"));
                r
            }
        }
    }

    /// representation-level conversion (the checker already validated every
    /// cast and resolved literal adaptations; this only changes form)
    fn convert(&mut self, v: &str, from: &QT, to: &QT, span: Span) -> String {
        if from == to {
            return v.to_string();
        }
        match (from, to) {
            // pointer-width integers and pointers are all 64-bit bits
            (QT::Int { bits: 64, .. } | QT::Str | QT::Ptr, QT::Int { bits: 64, .. } | QT::Str | QT::Ptr) => {
                v.to_string()
            }
            // arrays decay to pointers (the value already is the address)
            (QT::Array(..), QT::Ptr) => v.to_string(),
            // canonical bool is 0/1 in a word
            (QT::Bool, QT::Int { bits: 64, .. }) | (QT::Bool, QT::Int { bits: 32, .. }) => v.to_string(),
            (QT::Bool, QT::Int { bits, .. }) => {
                let _ = bits;
                let r = self.tmp();
                self.emit(format!("{r} =w extub {v}"));
                r
            }
            (QT::Int { bits, .. }, QT::Bool) => {
                let b = if *bits == 64 { "l" } else { "w" };
                let r = self.tmp();
                self.emit(format!("{r} =w cne{b} {v}, 0"));
                r
            }
            (QT::Int { bits: a, signed: sa }, QT::Int { bits: b, signed: sb }) => {
                let r = self.tmp();
                if *b < 32 {
                    // narrow target: truncate + canonicalize in one ext
                    let insn = match (*b, *sb) {
                        (8, true) => "extsb",
                        (8, false) => "extub",
                        (16, true) => "extsh",
                        _ => "extuh",
                    };
                    self.emit(format!("{r} =w {insn} {v}"));
                } else if *b == 64 && *a < 64 {
                    // widen to l by source width+signedness
                    let insn = match (*a, *sa) {
                        (8, true) => "extsb",
                        (8, false) => "extub",
                        (16, true) => "extsh",
                        (16, false) => "extuh",
                        (32, true) => "extsw",
                        _ => "extuw",
                    };
                    self.emit(format!("{r} =l {insn} {v}"));
                } else {
                    // w-to-w (incl. 64->32 truncation via subtyping): bits pass
                    return v.to_string();
                }
                r
            }
            (QT::Int { bits, signed }, QT::Float { bits: fb }) => {
                let r = self.tmp();
                let insn = match (*bits, *signed) {
                    (64, true) => "sltof",
                    (64, false) => "ultof",
                    (_, true) => "swtof",
                    (_, false) => "uwtof",
                };
                let base = if *fb == 64 { "d" } else { "s" };
                self.emit(format!("{r} ={base} {insn} {v}"));
                r
            }
            (QT::Float { bits: fb }, QT::Int { bits, signed }) => {
                let r = self.tmp();
                let insn = match (*fb, *signed) {
                    (32, true) => "stosi",
                    (32, false) => "stoui",
                    (_, true) => "dtosi",
                    (_, false) => "dtoui",
                };
                let base = if *bits == 64 { "l" } else { "w" };
                self.emit(format!("{r} ={base} {insn} {v}"));
                r
            }
            (QT::Float { bits: 32 }, QT::Float { bits: 64 }) => {
                let r = self.tmp();
                self.emit(format!("{r} =d exts {v}"));
                r
            }
            (QT::Float { bits: 64 }, QT::Float { bits: 32 }) => {
                let r = self.tmp();
                self.emit(format!("{r} =s truncd {v}"));
                r
            }
            _ => {
                self.fail(span, format!("qbe backend: unsupported conversion {from:?} -> {to:?}"));
                "%err".into()
            }
        }
    }

    /// intern a string literal into a data definition; returns its symbol
    fn str_sym(&mut self, s: &str) -> String {
        if let Some((_, sym)) = self.strs.iter().find(|(t, _)| t == s) {
            return sym.clone();
        }
        let sym = format!("$wlel_str{}", self.strs.len());
        let mut items: Vec<String> = Vec::new();
        let mut run = String::new();
        for b in s.bytes() {
            if (0x20..0x7f).contains(&b) && b != b'"' && b != b'\\' {
                run.push(b as char);
            } else {
                if !run.is_empty() {
                    items.push(format!("b \"{run}\""));
                    run.clear();
                }
                items.push(format!("b {b}"));
            }
        }
        if !run.is_empty() {
            items.push(format!("b \"{run}\""));
        }
        items.push("b 0".into());
        self.data.push(format!("data {sym} = {{ {} }}", items.join(", ")));
        self.strs.push((s.to_string(), sym.clone()));
        sym
    }

    /// load the value of a variable; arrays decay to their slot address
    fn read_var(&mut self, name: &str, ty: &QT, span: Span) -> (String, QT) {
        match self.lookup(name) {
            Some(Binding::Tmp(t)) => (t, ty.clone()),
            Some(Binding::Slot { addr, ty }) => {
                if matches!(ty, QT::Array(..)) {
                    return (addr, ty.clone());
                }
                let r = self.tmp();
                let op = load_op(&ty).unwrap_or("loadl");
                let base = ty.base().unwrap_or("l");
                self.emit(format!("{r} ={base} {op} {addr}"));
                (r, ty)
            }
            None => {
                self.fail(span, format!("qbe backend: unknown variable '{name}'"));
                ("%err".into(), ty.clone())
            }
        }
    }

    /// evaluate the address of an lvalue (`&`, assignment targets, array
    /// elements); returns (address, element type)
    fn addr_of(&mut self, e: &Expr) -> (String, QT) {
        let elem = QT::parse(e.ty.as_deref().unwrap_or("int")).unwrap_or(QT::Int { bits: 64, signed: true });
        match &e.node {
            ExprKind::Ident(n) => match self.lookup(n) {
                Some(Binding::Slot { addr, ty }) => (addr, ty),
                _ => {
                    self.fail(e.span, format!("qbe backend: cannot take the address of '{n}' here"));
                    ("%err".into(), elem)
                }
            },
            ExprKind::Deref(p) => {
                let (pv, _) = self.gen_expr(p);
                (pv, elem)
            }
            ExprKind::Index(b, i) => {
                let (base, _) = self.gen_expr(b);
                let (iv, ity) = self.gen_expr(i);
                let off = self.index_offset(&iv, &ity, &elem, i.span);
                let a = self.tmp();
                self.emit(format!("{a} =l add {base}, {off}"));
                (a, elem)
            }
            _ => {
                self.fail(e.span, "qbe backend: unsupported lvalue form");
                ("%err".into(), elem)
            }
        }
    }

    /// byte offset for `base[i]` as a long temporary
    fn index_offset(&mut self, iv: &str, ity: &QT, elem: &QT, span: Span) -> String {
        let i64v = self.convert(iv, ity, &QT::Int { bits: 64, signed: true }, span);
        match elem.size() {
            1 => i64v,
            n => {
                let s = self.tmp();
                self.emit(format!("{s} =l mul {i64v}, {n}"));
                s
            }
        }
    }

    // -- expressions ------------------------------------------------------

    fn gen_expr(&mut self, e: &Expr) -> (String, QT) {
        let want = self.ty_of(e);
        match &e.node {
            ExprKind::Int(v) => ((*v).to_string(), want),
            ExprKind::UInt(v) => ((*v as i64).to_string(), want),
            ExprKind::Float(f) => {
                let lit = if matches!(want, QT::Float { bits: 32 }) {
                    format!("s_{}", *f as f32)
                } else {
                    format!("d_{}", f)
                };
                (lit, want)
            }
            ExprKind::Bool(b) => ((if *b { "1" } else { "0" }).to_string(), QT::Bool),
            ExprKind::Str(s) => (self.str_sym(s), QT::Str),
            ExprKind::Ident(n) => self.read_var(n, &want, e.span),
            ExprKind::Cast(tname, inner) => {
                let Some(to) = QT::parse(tname) else {
                    self.fail(e.span, format!("qbe backend: unsupported cast target '{tname}'"));
                    return ("%err".into(), want);
                };
                let (v, from) = self.gen_expr(inner);
                (self.convert(&v, &from, &to, e.span), to)
            }
            ExprKind::Unary(op, x) => {
                let (v, ty) = self.gen_expr(x);
                let b = ty.base().unwrap_or("w");
                match op {
                    UnOp::Neg => {
                        let r = self.tmp();
                        self.emit(format!("{r} ={b} neg {v}"));
                        (self.narrow_fixup(&r, &ty), ty)
                    }
                    UnOp::Not => {
                        let r = self.tmp();
                        self.emit(format!("{r} =w xor {v}, 1"));
                        (r, QT::Bool)
                    }
                    UnOp::BitNot => {
                        let r = self.tmp();
                        self.emit(format!("{r} ={b} xor {v}, -1"));
                        (self.narrow_fixup(&r, &ty), ty)
                    }
                }
            }
            ExprKind::Binary(op, l, r) => self.gen_binop(*op, l, r, e.span),
            ExprKind::AddrOf(x) => {
                let (a, _) = self.addr_of(x);
                (a, QT::Ptr)
            }
            ExprKind::Deref(p) => {
                let (pv, _) = self.gen_expr(p);
                let Some(op) = load_op(&want) else {
                    self.fail(e.span, "qbe backend: cannot load a value of this type");
                    return ("%err".into(), want);
                };
                let r = self.tmp();
                let base = want.base().unwrap_or("l");
                self.emit(format!("{r} ={base} {op} {pv}"));
                (r, want)
            }
            ExprKind::Index(..) => {
                let (a, elem) = self.addr_of(e);
                let Some(op) = load_op(&elem) else {
                    self.fail(e.span, "qbe backend: cannot load a value of this type");
                    return ("%err".into(), want);
                };
                let r = self.tmp();
                let base = elem.base().unwrap_or("l");
                self.emit(format!("{r} ={base} {op} {a}"));
                (r, elem)
            }
            ExprKind::ArrayLit(items) => self.gen_array_lit(items, &want, e.span),
            ExprKind::Sizeof(tname) => {
                let Some(t) = QT::parse(tname) else {
                    self.fail(
                        e.span,
                        format!("qbe backend: wlel_sizeof of unsupported type '{tname}'"),
                    );
                    return ("%err".into(), want);
                };
                (t.size().to_string(), QT::Int { bits: 64, signed: false })
            }
            ExprKind::Call(name, type_args, args) => self.gen_call(e, name, type_args, args),
            other => {
                self.fail(e.span, describe_expr(other));
                ("%err".into(), want)
            }
        }
    }

    fn ty_of(&self, e: &Expr) -> QT {
        QT::parse(e.ty.as_deref().unwrap_or("int")).unwrap_or(QT::Int { bits: 64, signed: true })
    }

    fn gen_array_lit(&mut self, items: &[Expr], arr_ty: &QT, span: Span) -> (String, QT) {
        let slot = self.alloc(arr_ty.size());
        let elem = match arr_ty {
            QT::Array(e, _) => (**e).clone(),
            _ => {
                self.fail(span, "qbe backend: array literal with non-array type");
                return ("%err".into(), arr_ty.clone());
            }
        };
        for (i, item) in items.iter().enumerate() {
            let (v, vty) = self.gen_expr(item);
            let cv = self.convert(&v, &vty, &elem, item.span);
            let a = self.tmp();
            self.emit(format!("{a} =l add {slot}, {}", i * elem.size()));
            let op = store_op(&elem).unwrap_or("storel");
            self.emit(format!("{op} {cv}, {a}"));
        }
        (slot, arr_ty.clone())
    }

    fn gen_binop(&mut self, op: BinOp, l: &Expr, r: &Expr, span: Span) -> (String, QT) {
        let (lv, lt) = self.gen_expr(l);
        let out_ty = self.ty_of(l);
        // short-circuit logical operators: bools, evaluated via branches
        if matches!(op, BinOp::And | BinOp::Or) {
            let short = self.lbl("sc");
            let rhs = self.lbl("rhs");
            let join = self.lbl("scj");
            let res = self.tmp();
            if op == BinOp::And {
                self.term_line(format!("jnz {lv}, {rhs}, {short}"));
                self.label(&short);
                self.emit(format!("{res} =w copy 0"));
                self.term_line(format!("jmp {join}"));
            } else {
                self.term_line(format!("jnz {lv}, {short}, {rhs}"));
                self.label(&short);
                self.emit(format!("{res} =w copy 1"));
                self.term_line(format!("jmp {join}"));
            }
            self.label(&rhs);
            let (rv, rt) = self.gen_expr(r);
            let cv = self.convert(&rv, &rt, &QT::Bool, r.span);
            self.emit(format!("{res} =w copy {cv}"));
            self.label(&join);
            let _ = lt;
            return (res, QT::Bool);
        }
        let (rv, _rt) = self.gen_expr(r);
        match &out_ty {
            QT::Float { bits } => {
                let b = if *bits == 64 { "d" } else { "s" };
                let name = match op {
                    BinOp::Add => "add",
                    BinOp::Sub => "sub",
                    BinOp::Mul => "mul",
                    BinOp::Div => "div",
                    BinOp::Eq => "ceq",
                    BinOp::Ne => "cne",
                    BinOp::Lt => "clt",
                    BinOp::Le => "cle",
                    BinOp::Gt => "cgt",
                    BinOp::Ge => "cge",
                    other => {
                        self.fail(span, format!("qbe backend: operator {other:?} on float is outside the subset"));
                        return ("%err".into(), out_ty);
                    }
                };
                let x = self.tmp();
                if matches!(
                    op,
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                ) {
                    self.emit(format!("{x} =w {name}{b} {lv}, {rv}"));
                    (x, QT::Bool)
                } else {
                    self.emit(format!("{x} ={b} {name} {lv}, {rv}"));
                    (x, out_ty)
                }
            }
            QT::Int { bits, signed } => {
                let b = if *bits == 64 { "l" } else { "w" };
                let sfx = if *signed { "s" } else { "u" };
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        let name = match (op, *signed) {
                            (BinOp::Add, _) => "add",
                            (BinOp::Sub, _) => "sub",
                            (BinOp::Mul, _) => "mul",
                            (BinOp::Div, true) | (BinOp::Mod, true) => {
                                if op == BinOp::Div { "div" } else { "rem" }
                            }
                            _ => {
                                if op == BinOp::Div { "udiv" } else { "urem" }
                            }
                        };
                        let x = self.tmp();
                        self.emit(format!("{x} ={b} {name} {lv}, {rv}"));
                        (self.narrow_fixup(&x, &out_ty), out_ty)
                    }
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                        let name = match (op, *signed) {
                            (BinOp::BitAnd, _) => "and",
                            (BinOp::BitOr, _) => "or",
                            (BinOp::BitXor, _) => "xor",
                            (BinOp::Shl, _) => "shl",
                            (BinOp::Shr, true) => "sar",
                            _ => "shr",
                        };
                        let x = self.tmp();
                        self.emit(format!("{x} ={b} {name} {lv}, {rv}"));
                        (self.narrow_fixup(&x, &out_ty), out_ty)
                    }
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        // eq/ne are unsigned of sign; only the ordering
                        // relations carry the s/u prefix in QBE's names
                        let rel = match op {
                            BinOp::Eq => "eq".to_string(),
                            BinOp::Ne => "ne".to_string(),
                            BinOp::Lt => format!("{sfx}lt"),
                            BinOp::Le => format!("{sfx}le"),
                            BinOp::Gt => format!("{sfx}gt"),
                            _ => format!("{sfx}ge"),
                        };
                        let x = self.tmp();
                        self.emit(format!("{x} =w c{rel}{b} {lv}, {rv}"));
                        (x, QT::Bool)
                    }
                    BinOp::And | BinOp::Or => unreachable!(),
                }
            }
            QT::Bool => {
                let rel = match op {
                    BinOp::Eq => "eq",
                    BinOp::Ne => "ne",
                    _ => {
                        self.fail(span, "qbe backend: invalid operator on bool");
                        return ("%err".into(), out_ty);
                    }
                };
                let x = self.tmp();
                self.emit(format!("{x} =w c{rel}w {lv}, {rv}"));
                (x, QT::Bool)
            }
            QT::Str | QT::Ptr => {
                let rel = match op {
                    BinOp::Eq => "eq",
                    BinOp::Ne => "ne",
                    _ => {
                        self.fail(span, "qbe backend: invalid operator on pointer");
                        return ("%err".into(), out_ty);
                    }
                };
                let x = self.tmp();
                self.emit(format!("{x} =w c{rel}l {lv}, {rv}"));
                (x, QT::Bool)
            }
            _ => {
                self.fail(span, "qbe backend: invalid operand type");
                ("%err".into(), out_ty)
            }
        }
    }

    fn gen_call(&mut self, e: &Expr, name: &str, type_args: &[String], args: &[Expr]) -> (String, QT) {
        if !type_args.is_empty() {
            self.fail(e.span, "qbe backend: generics are outside the experiment subset");
            return ("%err".into(), QT::Void);
        }
        let want = self.ty_of(e);
        // bare runtime builtins (available without `use std`)
        match name {
            "wlel_print_int" | "wlel_print_float" | "wlel_print_str" => {
                let (v, _) = self.gen_expr(&args[0]);
                let b = if name == "wlel_print_float" { "d" } else { "l" };
                let short = name.trim_start_matches("wlel_print_");
                self.emit(format!("call $wlel_print_{short}({b} {v})"));
                return ("%void".into(), QT::Void);
            }
            "sys::argc" => {
                let r = self.tmp();
                self.emit(format!("{r} =l call $wlel_rt_argc()"));
                return (r, QT::Int { bits: 64, signed: true });
            }
            "sys::arg" => {
                let (v, vty) = self.gen_expr(&args[0]);
                let cv = self.convert(&v, &vty, &QT::Int { bits: 64, signed: true }, args[0].span);
                let r = self.tmp();
                self.emit(format!("{r} =l call $wlel_rt_arg(l {cv})"));
                return (r, QT::Str);
            }
            "sys::exit" => {
                let (v, vty) = self.gen_expr(&args[0]);
                let cv = self.convert(&v, &vty, &QT::Int { bits: 64, signed: true }, args[0].span);
                self.emit(format!("call $sys__exit(l {cv})"));
                self.term_line("hlt");
                return ("%void".into(), QT::Void);
            }
            "sys::mono_ms" | "sys::unix_ms" => {
                let f = if name == "sys::mono_ms" { "wlel_rt_mono_ms" } else { "wlel_rt_unix_ms" };
                let r = self.tmp();
                self.emit(format!("{r} =l call ${f}()"));
                return (r, QT::Int { bits: 64, signed: true });
            }
            "_wlel_streq" => {
                let (a, _) = self.gen_expr(&args[0]);
                let (b, _) = self.gen_expr(&args[1]);
                let r = self.tmp();
                self.emit(format!("{r} =w call $wlel_streq(l {a}, l {b})"));
                return (r, QT::Bool);
            }
            _ => {}
        }
        if name.starts_with("wlel_arena")
            || name.starts_with("wlel_alloc")
            || name.starts_with("wlel_free")
            || name.starts_with("std::")
            || name.starts_with("sys::")
        {
            self.fail(
                e.span,
                format!("'{name}' is outside the qbe experiment subset (needs std/arena runtime — use the C backend)"),
            );
            return ("%err".into(), want);
        }
        // user or extern function
        let Some((param_tys, ret_ty)) = self.fns.get(name).cloned() else {
            self.fail(e.span, format!("qbe backend: call to undefined function '{name}'"));
            return ("%err".into(), want);
        };
        if args.len() != param_tys.len() {
            self.fail(e.span, format!("qbe backend: argument count mismatch calling '{name}'"));
            return ("%err".into(), want);
        }
        let sym = if name == "main" { "wlel_user_main" } else { name };
        let mut parts = Vec::new();
        for (a, pty) in args.iter().zip(&param_tys) {
            let (v, vty) = self.gen_expr(a);
            let cv = self.convert(&v, &vty, pty, a.span);
            let abi = pty.abi().unwrap_or_else(|| "l".into());
            parts.push(format!("{abi} {cv}"));
        }
        let call = format!("call ${sym}({})", parts.join(", "));
        if matches!(ret_ty, QT::Void) {
            self.emit(call);
            return ("%void".into(), QT::Void);
        }
        let r = self.tmp();
        let base = ret_ty.base().unwrap_or("l");
        self.emit(format!("{r} ={base} {call}"));
        // sub-word returns leave the upper bits unspecified: canonicalize
        let r = if ret_ty.narrow_bits().is_some() || matches!(ret_ty, QT::Bool) {
            let (bits, signed) = ret_ty.narrow_bits().unwrap_or((8, false));
            let insn = match (bits, signed) {
                (8, true) => "extsb",
                (8, false) => "extub",
                (16, true) => "extsh",
                _ => "extuh",
            };
            let c = self.tmp();
            self.emit(format!("{c} =w {insn} {r}"));
            c
        } else {
            r
        };
        (r, ret_ty)
    }

    // -- statements -------------------------------------------------------

    fn gen_stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            if self.err.is_some() {
                return;
            }
            self.gen_stmt(s);
        }
    }

    /// unreachable code after a terminator flows into a fresh (dead) block
    fn dead_block(&mut self) {
        let l = self.lbl("dead");
        self.label(&l);
    }

    fn gen_stmt(&mut self, s: &Stmt) {
        if self.term {
            self.dead_block();
        }
        match &s.node {
            StmtKind::Let(name, _, init) => {
                let ty = self.ty_of(init);
                if matches!(ty, QT::Void) {
                    self.fail(s.span, "qbe backend: cannot bind void");
                    return;
                }
                let needs_slot = self.addr_taken.contains(name) || matches!(ty, QT::Array(..));
                if needs_slot {
                    let slot = self.alloc(ty.size());
                    if matches!(ty, QT::Array(..)) {
                        if let ExprKind::ArrayLit(items) = &init.node {
                            self.gen_array_items(items, &slot, &ty);
                        } else {
                            self.fail(init.span, "qbe backend: array bindings must be initialized with an array literal in the subset");
                        }
                    } else {
                        let (v, vty) = self.gen_expr(init);
                        let cv = self.convert(&v, &vty, &ty, init.span);
                        let op = store_op(&ty).unwrap_or("storel");
                        self.emit(format!("{op} {cv}, {slot}"));
                    }
                    self.bind(name, Binding::Slot { addr: slot, ty });
                } else {
                    let (v, vty) = self.gen_expr(init);
                    let cv = self.convert(&v, &vty, &ty, init.span);
                    let t = self.tmp();
                    let b = ty.base().unwrap_or("l");
                    self.emit(format!("{t} ={b} copy {cv}"));
                    self.bind(name, Binding::Tmp(t));
                }
            }
            StmtKind::Assign(a) => self.gen_assign(a),
            StmtKind::If(i) => self.gen_if(i),
            StmtKind::While(cond, body) => {
                let top = self.here("while");
                let bodyl = self.lbl("body");
                let endl = self.lbl("endwhile");
                let (c, _) = self.gen_expr(cond);
                self.term_line(format!("jnz {c}, {bodyl}, {endl}"));
                self.label(&bodyl);
                self.breaks.push(endl.clone());
                self.conts.push(top.clone());
                self.scopes.push(Default::default());
                self.gen_stmts(&body.0);
                self.scopes.pop();
                self.breaks.pop();
                self.conts.pop();
                if !self.term {
                    self.term_line(format!("jmp {top}"));
                }
                self.label(&endl);
            }
            StmtKind::For(var, start, end, body) => {
                let (sv, sty) = self.gen_expr(start);
                let (ev, ety) = self.gen_expr(end);
                let i64t = QT::Int { bits: 64, signed: true };
                let scv = self.convert(&sv, &sty, &i64t, start.span);
                let ecv = self.convert(&ev, &ety, &i64t, end.span);
                let varv = self.tmp();
                self.emit(format!("{varv} =l copy {scv}"));
                // the loop variable lives in a register (or a slot when its
                // address is taken); either way it is one binding per loop
                let slot_addr = if self.addr_taken.contains(var) {
                    let slot = self.alloc(8);
                    self.emit(format!("storel {varv}, {slot}"));
                    Some(slot)
                } else {
                    None
                };
                let cond = self.here("for");
                let bodyl = self.lbl("body");
                let stepl = self.lbl("step");
                let endl = self.lbl("endfor");
                let c = self.tmp();
                match &slot_addr {
                    Some(addr) => {
                        let cur = self.tmp();
                        self.emit(format!("{cur} =l loadl {addr}"));
                        self.emit(format!("{c} =w csltl {cur}, {ecv}"));
                    }
                    None => self.emit(format!("{c} =w csltl {varv}, {ecv}")),
                }
                self.term_line(format!("jnz {c}, {bodyl}, {endl}"));
                self.label(&bodyl);
                self.breaks.push(endl.clone());
                self.conts.push(stepl.clone());
                self.scopes.push(Default::default());
                match &slot_addr {
                    Some(addr) => {
                        self.bind(var, Binding::Slot { addr: addr.clone(), ty: i64t.clone() });
                    }
                    None => self.bind(var, Binding::Tmp(varv.clone())),
                }
                self.gen_stmts(&body.0);
                self.scopes.pop();
                self.breaks.pop();
                self.conts.pop();
                if !self.term {
                    self.term_line(format!("jmp {stepl}"));
                }
                self.label(&stepl);
                match &slot_addr {
                    Some(addr) => {
                        let r = self.tmp();
                        self.emit(format!("{r} =l loadl {addr}"));
                        let inc = self.tmp();
                        self.emit(format!("{inc} =l add {r}, 1"));
                        self.emit(format!("storel {inc}, {addr}"));
                    }
                    None => {
                        let inc = self.tmp();
                        self.emit(format!("{inc} =l add {varv}, 1"));
                        self.emit(format!("{varv} =l copy {inc}"));
                    }
                }
                self.term_line(format!("jmp {cond}"));
                self.label(&endl);
            }
            StmtKind::Break => match self.breaks.last().cloned() {
                Some(l) => self.term_line(format!("jmp {l}")),
                None => self.fail(s.span, "qbe backend: break outside loop"),
            },
            StmtKind::Continue => match self.conts.last().cloned() {
                Some(l) => self.term_line(format!("jmp {l}")),
                None => self.fail(s.span, "qbe backend: continue outside loop"),
            },
            StmtKind::Return(e) => {
                let ret_ty = self.cur_ret.clone();
                match e {
                    Some(x) => {
                        let (v, vty) = self.gen_expr(x);
                        if matches!(ret_ty, QT::Void) {
                            self.fail(s.span, "qbe backend: return value in void function");
                        } else {
                            let cv = self.convert(&v, &vty, &ret_ty, x.span);
                            self.term_line(format!("ret {cv}"));
                        }
                    }
                    None => {
                        if matches!(ret_ty, QT::Void) {
                            self.term_line("ret");
                        } else {
                            let zero = match &ret_ty {
                                QT::Float { bits: 32 } => "s_0",
                                QT::Float { bits: 64 } => "d_0",
                                _ => "0",
                            };
                            self.term_line(format!("ret {zero}"));
                        }
                    }
                }
            }
            StmtKind::ExprStmt(x) => {
                self.gen_expr(x);
            }
            StmtKind::Block(b) => {
                self.scopes.push(Default::default());
                self.gen_stmts(&b.0);
                self.scopes.pop();
            }
            other => {
                self.fail(s.span, describe_stmt(other));
            }
        }
    }

    fn gen_array_items(&mut self, items: &[Expr], slot: &str, arr_ty: &QT) {
        let elem = match arr_ty {
            QT::Array(e, _) => (**e).clone(),
            _ => QT::Int { bits: 64, signed: true },
        };
        for (i, item) in items.iter().enumerate() {
            let (v, vty) = self.gen_expr(item);
            let cv = self.convert(&v, &vty, &elem, item.span);
            let a = self.tmp();
            self.emit(format!("{a} =l add {slot}, {}", i * elem.size()));
            let op = store_op(&elem).unwrap_or("storel");
            self.emit(format!("{op} {cv}, {a}"));
        }
    }

    fn gen_assign(&mut self, a: &AssignStmt) {
        // plain variable targets: assign through the binding (register temp
        // or slot); Index/Deref targets go through the address below
        if let ExprKind::Ident(n) = &a.target.node {
            if let Some(Binding::Tmp(t)) = self.lookup(n) {
                let t = t.clone();
                let ty = self.ty_of(&a.target);
                let (nv, nty) = self.gen_expr(&a.value);
                let cv = self.convert(&nv, &nty, &ty, a.value.span);
                if a.op == CompoundOp::Set {
                    let b = ty.base().unwrap_or("l");
                    self.emit(format!("{t} ={b} copy {cv}"));
                    return;
                }
                let name = match (a.op, &ty) {
                    (CompoundOp::Add, _) => "add",
                    (CompoundOp::Sub, _) => "sub",
                    (CompoundOp::Mul, _) => "mul",
                    (CompoundOp::Div, QT::Int { signed: false, .. }) => "udiv",
                    (CompoundOp::Div, _) => "div",
                    (CompoundOp::Mod, QT::Int { signed: false, .. }) => "urem",
                    (CompoundOp::Mod, QT::Int { .. }) => "rem",
                    (CompoundOp::Mod, _) => {
                        self.fail(a.value.span, "qbe backend: '%' on float is outside the subset");
                        return;
                    }
                    (CompoundOp::Set, _) => unreachable!(),
                };
                let b = ty.base().unwrap_or("l");
                let r = self.tmp();
                self.emit(format!("{r} ={b} {name} {t}, {cv}"));
                let r = self.narrow_fixup(&r, &ty);
                let nb = ty.base().unwrap_or("l");
                self.emit(format!("{t} ={nb} copy {r}"));
                return;
            }
        }
        // the target address is evaluated exactly once
        let (addr, ty) = self.addr_of(&a.target);
        let (nv, nty) = self.gen_expr(&a.value);
        let cv = self.convert(&nv, &nty, &ty, a.value.span);
        if a.op == CompoundOp::Set {
            let op = store_op(&ty).unwrap_or("storel");
            self.emit(format!("{op} {cv}, {addr}"));
            return;
        }
        let lop = load_op(&ty).unwrap_or("loadl");
        let old = self.tmp();
        let base = ty.base().unwrap_or("l");
        self.emit(format!("{old} ={base} {lop} {addr}"));
        let name = match (a.op, &ty) {
            (CompoundOp::Add, _) => "add",
            (CompoundOp::Sub, _) => "sub",
            (CompoundOp::Mul, _) => "mul",
            (CompoundOp::Div, QT::Int { signed: false, .. }) => "udiv",
            (CompoundOp::Div, _) => "div",
            (CompoundOp::Mod, QT::Int { signed: false, .. }) => "urem",
            (CompoundOp::Mod, QT::Int { .. }) => "rem",
            (CompoundOp::Mod, _) => {
                self.fail(a.value.span, "qbe backend: '%' on float is outside the subset");
                return;
            }
            (CompoundOp::Set, _) => unreachable!(),
        };
        let r = self.tmp();
        self.emit(format!("{r} ={base} {name} {old}, {cv}"));
        let r = self.narrow_fixup(&r, &ty);
        let op = store_op(&ty).unwrap_or("storel");
        self.emit(format!("{op} {r}, {addr}"));
    }

    fn gen_if(&mut self, i: &IfStmt) {
        let (c, _) = self.gen_expr(&i.cond);
        let thenl = self.lbl("then");
        let join = self.lbl("endif");
        match &i.else_branch {
            Some(ElseBranch::Block(b)) => {
                let elsel = self.lbl("else");
                self.term_line(format!("jnz {c}, {thenl}, {elsel}"));
                self.label(&thenl);
                self.scopes.push(Default::default());
                self.gen_stmts(&i.then_body.0);
                self.scopes.pop();
                if !self.term {
                    self.term_line(format!("jmp {join}"));
                }
                self.label(&elsel);
                self.scopes.push(Default::default());
                self.gen_stmts(&b.0);
                self.scopes.pop();
                if !self.term {
                    self.term_line(format!("jmp {join}"));
                }
            }
            Some(ElseBranch::If(inner)) => {
                let elsel = self.lbl("else");
                self.term_line(format!("jnz {c}, {thenl}, {elsel}"));
                self.label(&thenl);
                self.scopes.push(Default::default());
                self.gen_stmts(&i.then_body.0);
                self.scopes.pop();
                if !self.term {
                    self.term_line(format!("jmp {join}"));
                }
                self.label(&elsel);
                self.gen_if(inner);
                if !self.term {
                    self.term_line(format!("jmp {join}"));
                }
            }
            None => {
                self.term_line(format!("jnz {c}, {thenl}, {join}"));
                self.label(&thenl);
                self.scopes.push(Default::default());
                self.gen_stmts(&i.then_body.0);
                self.scopes.pop();
                if !self.term {
                    self.term_line(format!("jmp {join}"));
                }
            }
        }
        self.label(&join);
    }

    // -- functions --------------------------------------------------------

    fn gen_fn(&mut self, f: &FuncDef) {
        if f.is_extern {
            return; // declarations only: the linker provides the symbol
        }
        self.cur_file = f.file.clone();
        self.cur_ret = f.ret_type.as_deref().and_then(QT::parse).unwrap_or(QT::Void);
        self.addr_taken.clear();
        collect_addr_taken_block(&f.body, &mut self.addr_taken);
        let sym = if f.name == "main" { "wlel_user_main" } else { f.name.as_str() };
        let ret_abi = match &self.cur_ret {
            QT::Void => String::new(),
            t => format!("{} ", t.abi().unwrap_or_else(|| "l".into())),
        };
        let mut params_il = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            let ty = QT::parse(p.ty.as_deref().unwrap_or("int")).unwrap_or(QT::Int { bits: 64, signed: true });
            if matches!(ty, QT::Array(..)) {
                self.fail(p.span, "qbe backend: array parameters are outside the experiment subset (pass a pointer)");
                return;
            }
            let abi = ty.abi().unwrap_or_else(|| "l".into());
            params_il.push(format!("{abi} %p{i}"));
        }
        self.allocs.clear();
        self.fn_out = String::new();
        self.term = false;
        self.scopes = vec![Default::default()];
        self.breaks.clear();
        self.conts.clear();
        // bind params: extend sub-word forms, spill address-taken ones
        for (i, p) in f.params.iter().enumerate() {
            let ty = QT::parse(p.ty.as_deref().unwrap_or("int")).unwrap_or(QT::Int { bits: 64, signed: true });
            let pt = format!("%p{i}");
            let v = if ty.narrow_bits().is_some() || matches!(ty, QT::Bool) {
                let (bits, signed) = ty.narrow_bits().unwrap_or((8, false));
                let insn = match (bits, signed) {
                    (8, true) => "extsb",
                    (8, false) => "extub",
                    (16, true) => "extsh",
                    _ => "extuh",
                };
                let c = self.tmp();
                self.emit(format!("{c} =w {insn} {pt}"));
                c
            } else {
                pt
            };
            if self.addr_taken.contains(&p.name) {
                let slot = self.alloc(ty.size());
                let op = store_op(&ty).unwrap_or("storel");
                self.emit(format!("{op} {v}, {slot}"));
                self.bind(&p.name, Binding::Slot { addr: slot, ty });
            } else {
                self.bind(&p.name, Binding::Tmp(v));
            }
        }
        self.gen_stmts(&f.body.0);
        if !self.term {
            match &self.cur_ret {
                QT::Void => self.term_line("ret"),
                QT::Float { bits: 32 } => self.term_line("ret s_0"),
                QT::Float { bits: 64 } => self.term_line("ret d_0"),
                t => {
                    let _ = t;
                    self.term_line("ret 0");
                }
            }
        }
        // assemble: signature, entry block with the hoisted allocations,
        // then the generated body
        self.out.push_str(&format!("function {ret_abi}${sym}({}) {{\n@start\n", params_il.join(", ")));
        for (t, n) in &self.allocs {
            self.out.push_str(&format!("\t{t} =l alloc8 {n}\n"));
        }
        self.out.push_str(&self.fn_out);
        self.out.push_str("}\n\n");
    }
}

fn describe_expr(e: &ExprKind) -> String {
    let what = match e {
        ExprKind::MethodCall(..) => "method call",
        ExprKind::Field(..) => "field access",
        ExprKind::StructLit(..) => "struct literal",
        ExprKind::EnumLit(..) => "enum value",
        ExprKind::Match(..) => "match expression",
        ExprKind::Try(..) => "try operator '?'",
        ExprKind::New(..) => "new()",
        ExprKind::CurrentArena => "wlel_arena()",
        _ => "this expression",
    };
    format!("qbe backend: {what} is outside the experiment subset (use the C backend)")
}

fn describe_stmt(s: &StmtKind) -> String {
    let what = match s {
        StmtKind::Defer(_) => "defer",
        StmtKind::Arena(..) => "arena block",
        StmtKind::Match(..) => "match",
        StmtKind::ForIn(..) => "for-in loop",
        _ => "this statement",
    };
    format!("qbe backend: {what} is outside the experiment subset (use the C backend)")
}

fn collect_addr_taken_block(b: &Block, set: &mut std::collections::HashSet<String>) {
    for s in &b.0 {
        collect_addr_taken_stmt(s, set);
    }
}

fn collect_addr_taken_stmt(s: &Stmt, set: &mut std::collections::HashSet<String>) {
    match &s.node {
        StmtKind::Let(_, _, e) => collect_addr_taken_expr(e, set),
        StmtKind::Assign(a) => {
            collect_addr_taken_expr(&a.target, set);
            collect_addr_taken_expr(&a.value, set);
        }
        StmtKind::If(i) => {
            collect_addr_taken_expr(&i.cond, set);
            collect_addr_taken_block(&i.then_body, set);
            match &i.else_branch {
                Some(ElseBranch::Block(b)) => collect_addr_taken_block(b, set),
                Some(ElseBranch::If(inner)) => {
                    collect_addr_taken_expr(&inner.cond, set);
                    collect_addr_taken_block(&inner.then_body, set);
                    if let Some(ElseBranch::Block(b)) = &inner.else_branch {
                        collect_addr_taken_block(b, set);
                    }
                }
                None => {}
            }
        }
        StmtKind::While(c, b) => {
            collect_addr_taken_expr(c, set);
            collect_addr_taken_block(b, set);
        }
        StmtKind::For(_, a, b2, body) => {
            collect_addr_taken_expr(a, set);
            collect_addr_taken_expr(b2, set);
            collect_addr_taken_block(body, set);
        }
        StmtKind::ForIn(f) => {
            collect_addr_taken_expr(&f.iter, set);
            collect_addr_taken_block(&f.body, set);
        }
        StmtKind::Return(Some(e)) | StmtKind::ExprStmt(e) => collect_addr_taken_expr(e, set),
        StmtKind::Block(b) => collect_addr_taken_block(b, set),
        StmtKind::Defer(DeferBody::Expr(e)) => collect_addr_taken_expr(e, set),
        StmtKind::Defer(DeferBody::Block(b)) => collect_addr_taken_block(b, set),
        StmtKind::Arena(_, b) => collect_addr_taken_block(b, set),
        StmtKind::Match(m) => {
            collect_addr_taken_expr(&m.scrutinee, set);
            for arm in &m.arms {
                match &arm.body {
                    MatchBody::Expr(e) => collect_addr_taken_expr(e, set),
                    MatchBody::Block(b) => collect_addr_taken_block(b, set),
                }
            }
        }
        _ => {}
    }
}

fn collect_addr_taken_expr(e: &Expr, set: &mut std::collections::HashSet<String>) {
    if let ExprKind::AddrOf(x) = &e.node {
        if let ExprKind::Ident(n) = &x.node {
            set.insert(n.clone());
        }
    }
    match &e.node {
        ExprKind::Unary(_, x) | ExprKind::AddrOf(x) | ExprKind::Deref(x) | ExprKind::Cast(_, x) => {
            collect_addr_taken_expr(x, set)
        }
        ExprKind::Binary(_, a, b) => {
            collect_addr_taken_expr(a, set);
            collect_addr_taken_expr(b, set);
        }
        ExprKind::Call(_, _, args) | ExprKind::MethodCall(_, _, args) => {
            for a in args {
                collect_addr_taken_expr(a, set);
            }
        }
        ExprKind::Field(o, _) => collect_addr_taken_expr(o, set),
        ExprKind::Index(a, b) => {
            collect_addr_taken_expr(a, set);
            collect_addr_taken_expr(b, set);
        }
        ExprKind::ArrayLit(items) => {
            for i in items {
                collect_addr_taken_expr(i, set);
            }
        }
        ExprKind::StructLit(_, fields) => {
            for (_, v) in fields {
                collect_addr_taken_expr(v, set);
            }
        }
        ExprKind::Match(m) => {
            collect_addr_taken_expr(&m.scrutinee, set);
            for arm in &m.arms {
                match &arm.body {
                    MatchBody::Expr(ex) => collect_addr_taken_expr(ex, set),
                    MatchBody::Block(b) => collect_addr_taken_block(b, set),
                }
            }
        }
        ExprKind::Try(t) => collect_addr_taken_expr(&t.inner, set),
        ExprKind::New(_, Some(n)) => collect_addr_taken_expr(n, set),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// entry point

/// transpile a checked program to QBE IL. Fails (with a `file:line:col`
/// message) on anything outside the experiment subset instead of guessing.
pub fn gen_program_il(p: &Program) -> Result<String, String> {
    let mut q = Qb::new();
    if let Some(u) = p.uses.first() {
        let file = p.funcs.first().map(|f| f.file.as_str()).unwrap_or("");
        let what = if u.path.is_none() { "'use std'" } else { "file imports" };
        return Err(format!("{file}:{}: qbe backend: {what} are outside the experiment subset (single self-contained file, no std — use the C backend)", u.span));
    }
    if !p.structs.is_empty() {
        let s = &p.structs[0];
        return Err(format!("{}:{}: qbe backend: structs are outside the experiment subset", s.file, s.span));
    }
    if !p.enums.is_empty() {
        let e = &p.enums[0];
        return Err(format!("{}:{}: qbe backend: enums are outside the experiment subset", e.file, e.span));
    }
    if !p.impls.is_empty() {
        let i = &p.impls[0];
        return Err(format!("{}:{}: qbe backend: impl blocks are outside the experiment subset", i.file, i.span));
    }
    if !p.tests.is_empty() {
        let t = &p.tests[0];
        return Err(format!("{}:{}: qbe backend: test blocks are outside the experiment subset", t.file, t.span));
    }
    if !p.funcs.iter().any(|f| f.name == "main") {
        return Err("qbe backend: program has no 'main' function".into());
    }
    for f in &p.funcs {
        let params: Vec<QT> = f
            .params
            .iter()
            .map(|pa| QT::parse(pa.ty.as_deref().unwrap_or("int")).unwrap_or(QT::Int { bits: 64, signed: true }))
            .collect();
        let ret = f.ret_type.as_deref().and_then(QT::parse).unwrap_or(QT::Void);
        q.fns.insert(f.name.clone(), (params, ret));
    }
    for f in &p.funcs {
        q.cur_file = f.file.clone();
        q.gen_fn(f);
        if let Some(e) = &q.err {
            return Err(format!("{}:{}: {}", e.file, e.span, e.msg));
        }
    }
    // entry wrapper: hand argc/argv to the runtime, forward to user main
    let user_main_ret = q
        .fns
        .get("main")
        .map(|(_, r)| r.clone())
        .unwrap_or(QT::Int { bits: 64, signed: true });
    q.out.push_str("export function w $main(w %_argc, l %_argv) {\n@start\n");
    q.out.push_str("\tcall $wlel_rt_init(w %_argc, l %_argv)\n");
    match user_main_ret {
        QT::Void => q.out.push_str("\tcall $wlel_user_main()\n\tret 0\n}\n"),
        QT::Int { bits: 64, .. } | QT::Str | QT::Ptr => {
            q.out.push_str("\t%r =l call $wlel_user_main()\n\tret %r\n}\n")
        }
        _ => q.out.push_str("\t%r =w call $wlel_user_main()\n\tret %r\n}\n"),
    }
    let mut full = String::from("# generated by wlel --backend qbe\n");
    for d in &q.data {
        full.push_str(d);
        full.push('\n');
    }
    if !q.data.is_empty() {
        full.push('\n');
    }
    full.push_str(&q.out);
    Ok(full)
}

/// the small runtime every `--backend qbe` binary links against. All libc
/// contact is funneled here so the same IL links on POSIX and Windows.
pub const RUNTIME_C: &str = r#"/* wlel qbe runtime (experiment) */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifdef _WIN32
#include <io.h>
#include <fcntl.h>
#endif

static int wlel_rt_argc_g = 0;
static char **wlel_rt_argv_g = 0;

void wlel_rt_init(int argc, char **argv) {
#ifdef _WIN32
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
#endif
    wlel_rt_argc_g = argc;
    wlel_rt_argv_g = argv;
}

long long wlel_rt_argc(void) { return (long long)wlel_rt_argc_g; }

const char *wlel_rt_arg(long long i) {
    if (i < 0 || i >= wlel_rt_argc_g) return "";
    return wlel_rt_argv_g[i];
}

void wlel_print_int(long long v) { printf("%lld\n", v); }
void wlel_print_float(double v) { printf("%g\n", v); }
void wlel_print_str(const char *s) { fputs(s, stdout); }
int wlel_streq(const char *a, const char *b) { return strcmp(a, b) == 0; }
void sys__exit(long long code) { exit((int)code); }

long long wlel_rt_mono_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000 + (long long)ts.tv_nsec / 1000000;
}

long long wlel_rt_unix_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (long long)ts.tv_sec * 1000 + (long long)ts.tv_nsec / 1000000;
}
"#;
