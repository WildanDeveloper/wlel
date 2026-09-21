use crate::ast::*;
use crate::span::{Span, Spanned};
use crate::stdsrc::STD_FILE;
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
    /// a tagged enum value (same C layout as a struct: `_tag` + union)
    Enum(String),
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
            Type::Enum(n) => n.clone(),
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
    /// concrete enums (plain and instantiated generics): name -> variants,
    /// each variant as (name, payload types)
    enums: HashMap<String, Vec<(String, Vec<Type>)>>,
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
    /// enum names referenced anywhere (types, constructors, match)
    used_enums: HashSet<String>,
    /// variant name -> owning enum's source name ("Circle" -> "Shape";
    /// for generics the TEMPLATE name: "Ok" -> "Result")
    variant_owner: HashMap<String, String>,
    /// positive while checking a `defer { ... }` block: control flow out of
    /// it is forbidden, because the block runs during scope unwinding
    defer_depth: usize,
    /// statements after a return/break/continue in the current block
    block_terminated: bool,
    /// source file of the function/test currently being checked, so
    /// `assert` failures can be reported with file:line at runtime
    current_file: String,
    warnings: Vec<CheckWarning>,
    /// generic function templates, keyed by source name; monomorphized
    /// into concrete functions at each call site
    generic_funcs: HashMap<String, FuncDef>,
    /// generic struct templates, keyed by source name
    generic_structs: HashMap<String, StructDef>,
    /// generic enum templates, keyed by source name
    generic_enums: HashMap<String, EnumDef>,
    /// mangled struct name -> concrete type arguments (for inference)
    struct_args: HashMap<String, Vec<Type>>,
    /// mangled struct name -> the generic struct it was instantiated from
    struct_origin: HashMap<String, String>,
    /// mangled enum name -> concrete type arguments (expected-type hints)
    enum_args: HashMap<String, Vec<Type>>,
    /// mangled enum name -> the generic enum it was instantiated from
    enum_origin: HashMap<String, String>,
    /// monomorphized function bodies awaiting their type check
    pending_funcs: Vec<FuncDef>,
    /// monomorphized struct definitions to append to the program
    pending_structs: Vec<StructDef>,
    /// monomorphized enum definitions to append to the program
    pending_enums: Vec<EnumDef>,
    /// expected-type hint for direct enum construction in an annotated
    /// `let`/`return`: (template enum name, concrete argument types) —
    /// lets `Ok(5)` fill in the `E` of `Result[int, E]` from the annotation
    expected: Option<(String, Vec<Type>)>,
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
            enums: HashMap::new(),
            scopes: vec![HashMap::new()],
            current_ret: Type::Void,
            loop_depth: 0,
            arena_depth: 0,
            use_std,
            std_called: false,
            called_funcs: HashSet::new(),
            used_structs: HashSet::new(),
            used_enums: HashSet::new(),
            variant_owner: HashMap::new(),
            defer_depth: 0,
            block_terminated: false,
            current_file: String::new(),
            warnings: Vec::new(),
            generic_funcs: HashMap::new(),
            generic_structs: HashMap::new(),
            generic_enums: HashMap::new(),
            struct_args: HashMap::new(),
            struct_origin: HashMap::new(),
            enum_args: HashMap::new(),
            enum_origin: HashMap::new(),
            pending_funcs: Vec::new(),
            pending_structs: Vec::new(),
            pending_enums: Vec::new(),
            expected: None,
        };

        // split generic templates out of the program: they are never checked
        // or emitted directly — each use produces a monomorphized copy
        let mut concrete_funcs = Vec::new();
        for f in std::mem::take(&mut program.funcs) {
            if f.type_params.is_empty() {
                concrete_funcs.push(f);
            } else {
                cx.register_generic_fn(f)?;
            }
        }
        program.funcs = concrete_funcs;
        let mut concrete_structs = Vec::new();
        for s in std::mem::take(&mut program.structs) {
            if s.type_params.is_empty() {
                concrete_structs.push(s);
            } else {
                cx.register_generic_struct(s)?;
            }
        }
        program.structs = concrete_structs;
        let mut concrete_enums = Vec::new();
        for e in std::mem::take(&mut program.enums) {
            if e.type_params.is_empty() {
                concrete_enums.push(e);
            } else {
                cx.register_generic_enum(e)?;
            }
        }
        program.enums = concrete_enums;

        // pass 0: register struct names, then resolve field types.
        // 'ArenaStats' is pre-registered: it is the built-in result type of
        // arena_stats() and cannot be shadowed by a user struct. Same for
        // 'File', the opaque handle type of std::fs::open().
        cx.structs.insert(
            "ArenaStats".into(),
            vec![
                ("bytes".into(), Type::Int(IntW::I64)),
                ("chunks".into(), Type::Int(IntW::I64)),
                ("peak".into(), Type::Int(IntW::I64)),
            ],
        );
        cx.structs.insert(
            "File".into(),
            vec![("h".into(), Type::Ptr(Box::new(Type::Void)))],
        );
        for st in &program.structs {
            if cx.structs.insert(st.name.clone(), Vec::new()).is_some() {
                if st.name == "ArenaStats" {
                    return err_at(
                        st.span,
                        "struct 'ArenaStats' is reserved (built-in result type of arena_stats())",
                    );
                }
                if st.name == "File" {
                    return err_at(
                        st.span,
                        "struct 'File' is reserved (built-in handle type of std::fs::open())",
                    );
                }
                return err_at(st.span, format!("duplicate struct '{}'", st.name));
            }
            if cx.enums.contains_key(&st.name) {
                return err_at(
                    st.span,
                    format!("duplicate type '{}': already declared as an enum", st.name),
                );
            }
        }
        // enum names are part of the type namespace: register them before
        // any field/payload resolution so structs and enums can reference
        // each other's names freely
        for e in &program.enums {
            if cx.enums.insert(e.name.clone(), Vec::new()).is_some() {
                return err_at(e.span, format!("duplicate enum '{}'", e.name));
            }
            if cx.structs.contains_key(&e.name) {
                return err_at(
                    e.span,
                    format!("duplicate type '{}': already declared as a struct", e.name),
                );
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

        // pass 0b: resolve enum variant payload types
        for e in &program.enums {
            let mut variants = Vec::with_capacity(e.variants.len());
            for v in &e.variants {
                let mut payloads = Vec::with_capacity(v.payloads.len());
                for p in &v.payloads {
                    let t = cx.resolve_type_str(p, e.span)?;
                    if t == Type::Void {
                        return err_at(
                            e.span,
                            format!(
                                "enum '{}': variant '{}' cannot have a void payload",
                                e.name, v.name
                            ),
                        );
                    }
                    if matches!(t, Type::Array(..)) {
                        return err_at(
                            e.span,
                            format!(
                                "enum '{}': variant '{}' — array payloads are not supported, wrap the array in a struct",
                                e.name, v.name
                            ),
                        );
                    }
                    payloads.push(t);
                }
                variants.push((v.name.clone(), payloads));
            }
            cx.enums.insert(e.name.clone(), variants);
        }
        // variant namespace: globally unique, and distinct from struct and
        // enum names — a bare `Circle(x)` call must be unambiguous
        for e in &program.enums {
            for v in &e.variants {
                if cx.variant_owner.insert(v.name.clone(), e.name.clone()).is_some() {
                    return err_at(
                        e.span,
                        format!("duplicate variant '{}'", v.name),
                    );
                }
                if cx.structs.contains_key(&v.name) {
                    return err_at(
                        e.span,
                        format!(
                            "variant '{}' collides with struct '{}' — rename one",
                            v.name, v.name
                        ),
                    );
                }
            }
        }

        // pass 1: signatures
        for f in &program.funcs {
            if f.is_extern && f.name == "main" {
                return err_at(
                    f.span,
                    "the entry point 'main' must be defined in Wlel, not extern",
                );
            }
            if cx.variant_owner.contains_key(&f.name) {
                return err_at(
                    f.span,
                    format!(
                        "duplicate name '{}': a function cannot share a name with an enum variant",
                        f.name
                    ),
                );
            }
            if cx.generic_funcs.contains_key(&f.name) {
                return err_at(
                    f.span,
                    format!("duplicate function '{}': a generic with this name exists", f.name),
                );
            }
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

        // pass 2: bodies (extern declarations have none — they bind to
        // external C symbols at link time)
        for f in program.funcs.iter_mut() {
            if f.is_extern {
                continue;
            }
            cx.check_one_fn(f)?;
        }

        // pass 2b: test bodies — void functions with no params
        for t in program.tests.iter_mut() {
            cx.current_ret = Type::Void;
            cx.scopes = vec![HashMap::new()];
            cx.block_terminated = false;
            cx.current_file = t.file.clone();
            cx.check_block_mut(&mut t.body)?;
        }

        // pass 3: type-check every monomorphized function body; bodies may
        // call further generics, so drain the queue until it is empty
        let mut instantiated = Vec::new();
        while let Some(mut f) = cx.pending_funcs.pop() {
            cx.check_one_fn(&mut f)?;
            instantiated.push(f);
        }
        program.funcs.append(&mut instantiated);
        program.structs.append(&mut cx.pending_structs);
        program.enums.append(&mut cx.pending_enums);

        // pass 4: rewrite generic syntax in the concrete program's type
        // annotations into mangled struct names, so the codegen sees plain
        // C types ("Box[int]" -> "Box__int"); signatures included, since a
        // concrete function may return/take a generic struct (std's
        // `fn str_split(...) -> Vec[string]`); may instantiate more structs
        let empty_map = HashMap::new();
        for f in program.funcs.iter_mut() {
            if let Some(rt) = f.ret_type.as_mut() {
                *rt = cx.canon_type(rt, &empty_map, f.span)?;
            }
            for p in f.params.iter_mut() {
                if let Some(pt) = p.ty.as_mut() {
                    *pt = cx.canon_type(pt, &empty_map, f.span)?;
                }
            }
            cx.canon_block(&mut f.body, &empty_map, f.span)?;
        }
        for t in program.tests.iter_mut() {
            cx.canon_block(&mut t.body, &empty_map, t.span)?;
        }
        for s in program.structs.iter_mut() {
            for (_, fty) in s.fields.iter_mut() {
                *fty = cx.canon_type(fty, &empty_map, s.span)?;
            }
        }
        program.structs.append(&mut cx.pending_structs);
        program.enums.append(&mut cx.pending_enums);

        // unused imports: a file import is used when any function or struct
        // defined in it is referenced; `use std` when any std:: call exists
        let used_files: HashSet<String> = called_files(
            program,
            &cx.called_funcs,
            &cx.used_structs,
            &cx.used_enums,
            &cx.generic_funcs,
            &cx.generic_structs,
            &cx.generic_enums,
        );
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
        // bare calls into the embedded library (vec_new, str_find, ...)
        // mark `use std` as used too: the spliced functions carry the
        // <std> file tag, so a referenced one proves the import is live
        let std_touched = cx.std_called
            || program
                .funcs
                .iter()
                .any(|f| f.file == STD_FILE && cx.called_funcs.contains(&f.name))
            || program
                .structs
                .iter()
                .any(|s| s.file == STD_FILE && cx.used_structs.contains(&s.name))
            || program
                .enums
                .iter()
                .any(|e| e.file == STD_FILE && cx.used_enums.contains(&e.name));
        if use_std && !std_touched {
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

    /// shared per-function body check used by concrete functions and by
    /// every monomorphized generic instantiation (pass 2 and pass 3)
    fn check_one_fn(&mut self, f: &mut FuncDef) -> CResult<()> {
        let sig = self.sigs.get(&f.name).unwrap().clone();
        let sig_params = sig.params.clone();
        let sig_ret = sig.ret.clone();
        self.current_ret = sig_ret.clone();
        self.scopes = vec![HashMap::new()];
        self.block_terminated = false;
        self.current_file = f.file.clone();
        for (p, t) in f.params.iter().zip(sig_params.iter()) {
            self.declare(&f.name, &p.name, t.clone(), p.span)?;
        }
        self.check_block_mut(&mut f.body)?;
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
        self.report_unused_scope("parameter");
        self.scopes[0].clear();
        Ok(())
    }

    /// register a `fn name[T, ...]` template: type params must be fresh
    /// names and every parameter annotated, or inference would be impossible
    fn register_generic_fn(&mut self, f: FuncDef) -> CResult<()> {
        for tp in &f.type_params {
            if Type::from_builtin(tp).is_some() || tp == "ArenaStats" || tp == "File" {
                return err_at(
                    f.span,
                    format!("type parameter '{tp}' shadows a built-in type"),
                );
            }
        }
        let mut seen = HashSet::new();
        for tp in &f.type_params {
            if !seen.insert(tp.clone()) {
                return err_at(f.span, format!("duplicate type parameter '{tp}'"));
            }
        }
        for p in &f.params {
            if p.ty.is_none() {
                return err_at(
                    p.span,
                    format!(
                        "generic function '{}': parameter '{}' needs a type annotation",
                        f.name, p.name
                    ),
                );
            }
        }
        let span = f.span;
        if self.generic_funcs.insert(f.name.clone(), f).is_some() {
            return err_at(span, "duplicate function");
        }
        Ok(())
    }

    /// register a `struct Name[T, ...]` template
    fn register_generic_struct(&mut self, s: StructDef) -> CResult<()> {
        for tp in &s.type_params {
            if Type::from_builtin(tp).is_some() || tp == "ArenaStats" || tp == "File" {
                return err_at(s.span, format!("type parameter '{tp}' shadows a built-in type"));
            }
        }
        let mut seen = HashSet::new();
        for tp in &s.type_params {
            if !seen.insert(tp.clone()) {
                return err_at(s.span, format!("duplicate type parameter '{tp}'"));
            }
        }
        let span = s.span;
        if self.generic_structs.insert(s.name.clone(), s).is_some() {
            return err_at(span, "duplicate struct");
        }
        Ok(())
    }

    /// register an `enum Name[T, ...]` template; its variant names enter the
    /// global variant namespace immediately (construction resolves by name)
    fn register_generic_enum(&mut self, e: EnumDef) -> CResult<()> {
        for tp in &e.type_params {
            if Type::from_builtin(tp).is_some() || tp == "ArenaStats" || tp == "File" {
                return err_at(e.span, format!("type parameter '{tp}' shadows a built-in type"));
            }
        }
        let mut seen = HashSet::new();
        for tp in &e.type_params {
            if !seen.insert(tp.clone()) {
                return err_at(e.span, format!("duplicate type parameter '{tp}'"));
            }
        }
        for v in &e.variants {
            if self.generic_funcs.contains_key(&v.name) {
                return err_at(
                    e.span,
                    format!(
                        "variant '{}' collides with generic function '{}' — rename one",
                        v.name, v.name
                    ),
                );
            }
            if self.variant_owner.insert(v.name.clone(), e.name.clone()).is_some() {
                return err_at(e.span, format!("duplicate variant '{}'", v.name));
            }
        }
        let span = e.span;
        if self.generic_enums.insert(e.name.clone(), e).is_some() {
            return err_at(span, "duplicate enum");
        }
        Ok(())
    }

    /// monomorphize a generic enum for one concrete argument list and
    /// register the result under its mangled name ("Result[int, E]" ->
    /// "Result__int__string")
    fn instantiate_enum(&mut self, name: &str, raw_args: Vec<String>, sp: Span) -> CResult<String> {
        let gen = self
            .generic_enums
            .get(name)
            .ok_or_else(|| CheckError {
                msg: format!("unknown generic enum '{name}'"),
                span: sp,
            })?
            .clone();
        if raw_args.len() != gen.type_params.len() {
            return err_at(
                sp,
                format!(
                    "enum '{name}' takes {} type argument(s), got {}",
                    gen.type_params.len(),
                    raw_args.len()
                ),
            );
        }
        let empty_map = HashMap::new();
        let mut args = Vec::with_capacity(raw_args.len());
        for a in &raw_args {
            args.push(self.canon_type(a, &empty_map, sp)?);
        }
        let mut map = HashMap::new();
        for (tp, a) in gen.type_params.iter().zip(args.iter()) {
            map.insert(tp.clone(), a.clone());
        }
        let mangled = format!(
            "{}__{}",
            name,
            args.iter().map(|a| mangle_ty(a)).collect::<Vec<_>>().join("__")
        );
        if self.enums.contains_key(&mangled) {
            return Ok(mangled);
        }
        let mut arg_types = Vec::with_capacity(args.len());
        for a in &args {
            arg_types.push(self.resolve_type_str(a, sp)?);
        }
        self.enums.insert(mangled.clone(), Vec::new());
        self.enum_args.insert(mangled.clone(), arg_types);
        self.enum_origin.insert(mangled.clone(), name.to_string());
        let mut variants = Vec::with_capacity(gen.variants.len());
        let mut concrete_variants = Vec::with_capacity(gen.variants.len());
        for v in &gen.variants {
            let mut payloads = Vec::with_capacity(v.payloads.len());
            let mut concrete_payloads = Vec::with_capacity(v.payloads.len());
            for p in &v.payloads {
                let concrete = self.canon_type(p, &map, sp)?;
                let t = self.resolve_type_str(&concrete, sp)?;
                if t == Type::Void {
                    return err_at(
                        sp,
                        format!("enum '{name}': variant '{}' cannot have a void payload", v.name),
                    );
                }
                if matches!(t, Type::Array(..)) {
                    return err_at(
                        sp,
                        format!(
                            "enum '{name}': variant '{}' — array payloads are not supported, wrap the array in a struct",
                            v.name
                        ),
                    );
                }
                payloads.push(t);
                concrete_payloads.push(concrete);
            }
            variants.push((v.name.clone(), payloads));
            concrete_variants.push(VariantDef {
                name: v.name.clone(),
                payloads: concrete_payloads,
            });
        }
        self.enums.insert(mangled.clone(), variants);
        self.pending_enums.push(EnumDef {
            name: mangled.clone(),
            variants: concrete_variants,
            type_params: Vec::new(),
            span: gen.span,
            file: gen.file.clone(),
        });
        self.used_enums.insert(name.to_string());
        Ok(mangled)
    }

    /// monomorphize a generic struct for one concrete argument list and
    /// register the result under its mangled name ("Box[int]" -> "Box__int")
    fn instantiate_struct(&mut self, name: &str, raw_args: Vec<String>, sp: Span) -> CResult<String> {
        let gen = self
            .generic_structs
            .get(name)
            .ok_or_else(|| CheckError {
                msg: format!("unknown generic struct '{name}'"),
                span: sp,
            })?
            .clone();
        if raw_args.len() != gen.type_params.len() {
            return err_at(
                sp,
                format!(
                    "struct '{name}' takes {} type argument(s), got {}",
                    gen.type_params.len(),
                    raw_args.len()
                ),
            );
        }
        // canonicalize arguments first ("Vec[int]" -> "Vec__int") so the
        // mangled name is stable no matter which syntax reached us
        let empty_map = HashMap::new();
        let mut args = Vec::with_capacity(raw_args.len());
        for a in &raw_args {
            args.push(self.canon_type(a, &empty_map, sp)?);
        }
        let mut map = HashMap::new();
        for (tp, a) in gen.type_params.iter().zip(args.iter()) {
            map.insert(tp.clone(), a.clone());
        }
        let mangled = format!(
            "{}__{}",
            name,
            args.iter().map(|a| mangle_ty(a)).collect::<Vec<_>>().join("__")
        );
        if self.structs.contains_key(&mangled) {
            return Ok(mangled);
        }
        // resolve the concrete argument types first (they may reference
        // other generics), then register a placeholder so recursive field
        // types like `next: *Node[T]` terminate
        let mut arg_types = Vec::with_capacity(args.len());
        for a in &args {
            arg_types.push(self.resolve_type_str(a, sp)?);
        }
        self.structs.insert(mangled.clone(), Vec::new());
        self.struct_args.insert(mangled.clone(), arg_types);
        self.struct_origin.insert(mangled.clone(), name.to_string());
        let mut fields = Vec::with_capacity(gen.fields.len());
        let mut concrete_fields = Vec::with_capacity(gen.fields.len());
        for (fname, fty) in &gen.fields {
            let concrete = self.canon_type(fty, &map, sp)?;
            let t = self.resolve_type_str(&concrete, sp)?;
            if t == Type::Void {
                return err_at(sp, format!("struct '{name}': field '{fname}' cannot be void"));
            }
            fields.push((fname.clone(), t));
            concrete_fields.push((fname.clone(), concrete));
        }
        self.structs.insert(mangled.clone(), fields);
        self.pending_structs.push(StructDef {
            name: mangled.clone(),
            fields: concrete_fields,
            type_params: Vec::new(),
            span: gen.span,
            file: gen.file.clone(),
        });
        self.used_structs.insert(name.to_string());
        Ok(mangled)
    }

    /// rewrite a type string into codegen-ready form: type parameters are
    /// substituted from `map`, and every `Name[args]` (generic struct use)
    /// becomes its mangled struct name ("*T" -> "*int", "Box[int]" ->
    /// "Box__int"). Instantiates generics as needed.
    fn canon_type(
        &mut self,
        ty: &str,
        map: &HashMap<String, String>,
        sp: Span,
    ) -> CResult<String> {
        let chars: Vec<char> = ty.chars().collect();
        let mut out = String::new();
        let mut ident = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_alphanumeric() || c == '_' {
                ident.push(c);
                i += 1;
                continue;
            }
            if !ident.is_empty() {
                if c == '[' && self.generic_structs.contains_key(&ident) {
                    // find the matching close bracket
                    let mut depth = 0usize;
                    let mut j = i;
                    while j < chars.len() {
                        if chars[j] == '[' {
                            depth += 1;
                        } else if chars[j] == ']' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        j += 1;
                    }
                    if j >= chars.len() {
                        return err_at(sp, format!("bad type '{ty}'"));
                    }
                    let inner: String = chars[i + 1..j].iter().collect();
                    let raw_args = split_top_level_args(&inner);
                    let mut args = Vec::with_capacity(raw_args.len());
                    for a in &raw_args {
                        args.push(self.canon_type(a, map, sp)?);
                    }
                    let mangled = self.instantiate_struct(&ident, args, sp)?;
                    out.push_str(&mangled);
                    ident.clear();
                    i = j + 1;
                    continue;
                }
                if c == '[' && self.generic_enums.contains_key(&ident) {
                    let mut depth = 0usize;
                    let mut j = i;
                    while j < chars.len() {
                        if chars[j] == '[' {
                            depth += 1;
                        } else if chars[j] == ']' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        j += 1;
                    }
                    if j >= chars.len() {
                        return err_at(sp, format!("bad type '{ty}'"));
                    }
                    let inner: String = chars[i + 1..j].iter().collect();
                    let raw_args = split_top_level_args(&inner);
                    let mut args = Vec::with_capacity(raw_args.len());
                    for a in &raw_args {
                        args.push(self.canon_type(a, map, sp)?);
                    }
                    let mangled = self.instantiate_enum(&ident, args, sp)?;
                    out.push_str(&mangled);
                    ident.clear();
                    i = j + 1;
                    continue;
                }
                out.push_str(map.get(&ident).unwrap_or(&ident));
                ident.clear();
            }
            out.push(c);
            i += 1;
        }
        if !ident.is_empty() {
            out.push_str(map.get(&ident).unwrap_or(&ident));
        }
        Ok(out)
    }

    /// canon_type across a whole monomorphized body (annotations only)
    fn canon_block(&mut self, b: &mut Block, map: &HashMap<String, String>, sp: Span) -> CResult<()> {
        for s in b.0.iter_mut() {
            self.canon_stmt(s, map, sp)?;
        }
        Ok(())
    }

    fn canon_stmt(&mut self, s: &mut Stmt, map: &HashMap<String, String>, sp: Span) -> CResult<()> {
        match &mut s.node {
            StmtKind::Let(_, ty_ann, e) => {
                if let Some(t) = ty_ann {
                    *t = self.canon_type(t, map, sp)?;
                }
                self.canon_expr(e, map, sp)?;
            }
            StmtKind::Assign(a) => {
                self.canon_expr(&mut a.target, map, sp)?;
                self.canon_expr(&mut a.value, map, sp)?;
            }
            StmtKind::If(i) => {
                self.canon_expr(&mut i.cond, map, sp)?;
                self.canon_block(&mut i.then_body, map, sp)?;
                match &mut i.else_branch {
                    Some(ElseBranch::Block(b)) => self.canon_block(b, map, sp)?,
                    Some(ElseBranch::If(inner)) => self.canon_if(inner, map, sp)?,
                    None => {}
                }
            }
            StmtKind::While(cond, body) => {
                self.canon_expr(cond, map, sp)?;
                self.canon_block(body, map, sp)?;
            }
            StmtKind::For(_, start, end, body) => {
                self.canon_expr(start, map, sp)?;
                self.canon_expr(end, map, sp)?;
                self.canon_block(body, map, sp)?;
            }
            StmtKind::ForIn(f) => {
                self.canon_expr(&mut f.iter, map, sp)?;
                self.canon_block(&mut f.body, map, sp)?;
            }
            StmtKind::Return(Some(e)) => self.canon_expr(e, map, sp)?,
            StmtKind::ExprStmt(e) => self.canon_expr(e, map, sp)?,
            StmtKind::Block(b) => self.canon_block(b, map, sp)?,
            StmtKind::Arena(cap, body) => {
                if let Some(c) = cap {
                    self.canon_expr(c, map, sp)?;
                }
                self.canon_block(body, map, sp)?;
            }
            StmtKind::Defer(DeferBody::Expr(e)) => self.canon_expr(e, map, sp)?,
            StmtKind::Defer(DeferBody::Block(b)) => self.canon_block(b, map, sp)?,
            StmtKind::Match(m) => {
                self.canon_expr(&mut m.scrutinee, map, sp)?;
                for arm in m.arms.iter_mut() {
                    match &mut arm.body {
                        MatchBody::Expr(x) => self.canon_expr(x, map, sp)?,
                        MatchBody::Block(b) => self.canon_block(b, map, sp)?,
                    }
                }
            }
            StmtKind::Break | StmtKind::Continue | StmtKind::Return(None) => {}
        }
        Ok(())
    }

    fn canon_if(&mut self, i: &mut IfStmt, map: &HashMap<String, String>, sp: Span) -> CResult<()> {
        self.canon_expr(&mut i.cond, map, sp)?;
        self.canon_block(&mut i.then_body, map, sp)?;
        match &mut i.else_branch {
            Some(ElseBranch::Block(b)) => self.canon_block(b, map, sp)?,
            Some(ElseBranch::If(inner)) => self.canon_if(inner, map, sp)?,
            None => {}
        }
        Ok(())
    }

    fn canon_expr(&mut self, e: &mut Expr, map: &HashMap<String, String>, sp: Span) -> CResult<()> {
        match &mut e.node {
            ExprKind::Cast(ty, x) => {
                *ty = self.canon_type(ty, map, sp)?;
                self.canon_expr(x, map, sp)?;
            }
            ExprKind::New(ty, cnt) => {
                *ty = self.canon_type(ty, map, sp)?;
                if let Some(c) = cnt {
                    self.canon_expr(c, map, sp)?;
                }
            }
            ExprKind::Sizeof(ty) => {
                *ty = self.canon_type(ty, map, sp)?;
            }
            ExprKind::Unary(_, x) => self.canon_expr(x, map, sp)?,
            ExprKind::Binary(_, l, r) => {
                self.canon_expr(l, map, sp)?;
                self.canon_expr(r, map, sp)?;
            }
            ExprKind::Call(_, ty_args, args) => {
                for t in ty_args.iter_mut() {
                    *t = self.canon_type(t, map, sp)?;
                }
                for a in args.iter_mut() {
                    self.canon_expr(a, map, sp)?;
                }
            }
            ExprKind::AddrOf(x) | ExprKind::Deref(x) => self.canon_expr(x, map, sp)?,
            ExprKind::Field(obj, _) => self.canon_expr(obj, map, sp)?,
            ExprKind::StructLit(_, fields) => {
                for (_, v) in fields.iter_mut() {
                    self.canon_expr(v, map, sp)?;
                }
            }
            ExprKind::Index(b, i) => {
                self.canon_expr(b, map, sp)?;
                self.canon_expr(i, map, sp)?;
            }
            ExprKind::ArrayLit(elems) => {
                for el in elems.iter_mut() {
                    self.canon_expr(el, map, sp)?;
                }
            }
            ExprKind::EnumLit(_, _, _, payloads) => {
                for p in payloads.iter_mut() {
                    self.canon_expr(p, map, sp)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// monomorphize a generic function for one binding of its type
    /// parameters ("id[int]") and queue the substituted body for checking
    fn instantiate_fn(
        &mut self,
        name: &str,
        binds: &HashMap<String, Type>,
        sp: Span,
    ) -> CResult<String> {
        let gen = self
            .generic_funcs
            .get(name)
            .ok_or_else(|| CheckError {
                msg: format!("unknown generic function '{name}'"),
                span: sp,
            })?
            .clone();
        let mut map = HashMap::new();
        for tp in &gen.type_params {
            let t = binds.get(tp).ok_or_else(|| CheckError {
                msg: format!("cannot infer type parameter '{tp}' of '{name}'"),
                span: sp,
            })?;
            map.insert(tp.clone(), t.name());
        }
        let mangled = format!(
            "{}__{}",
            name,
            gen.type_params
                .iter()
                .map(|tp| mangle_ty(&map[tp]))
                .collect::<Vec<_>>()
                .join("__")
        );
        if self.sigs.contains_key(&mangled) {
            return Ok(mangled); // already instantiated or queued
        }
        // register the concrete signature first so recursive calls inside
        // the body resolve to this instantiation instead of recursing here
        let mut params = Vec::with_capacity(gen.params.len());
        for p in &gen.params {
            let ty_str = self.canon_type(p.ty.as_deref().unwrap_or("int"), &map, p.span)?;
            params.push(self.resolve_type_str(&ty_str, p.span)?);
        }
        let ret_str = self.canon_type(
            gen.ret_type.as_deref().unwrap_or("void"),
            &map,
            gen.span,
        )?;
        let ret = self.resolve_type_str(&ret_str, gen.span)?;
        self.sigs.insert(
            mangled.clone(),
            FuncSig { params, ret },
        );
        // substitute annotations in the body, queue it for the normal pass
        let mut body_fn = gen;
        body_fn.name = mangled.clone();
        for p in body_fn.params.iter_mut() {
            if let Some(t) = p.ty.as_mut() {
                *t = self.canon_type(t, &map, sp)?;
            }
        }
        body_fn.ret_type = Some(self.canon_type(body_fn.ret_type.as_deref().unwrap_or("void"), &map, sp)?);
        self.canon_block(&mut body_fn.body, &map, sp)?;
        self.pending_funcs.push(body_fn);
        Ok(mangled)
    }

    /// unify a (possibly generic) type pattern against a concrete argument
    /// type, binding type variables; conflicting bindings are an error.
    /// `params` are the type parameters in scope for this pattern.
    fn unify_pattern(
        &self,
        pattern: &str,
        arg: &Type,
        params: &[String],
        binds: &mut HashMap<String, Type>,
        sp: Span,
    ) -> CResult<()> {
        let stars = pattern.chars().take_while(|c| *c == '*').count();
        let base_full = &pattern[stars..];
        // peel pointer layers on both sides
        let mut arg_inner = arg.clone();
        for _ in 0..stars {
            match arg_inner {
                Type::Ptr(i) => arg_inner = *i,
                other => {
                    return err_at(
                        sp,
                        format!("generic argument: expected '{pattern}', got '{}'", other.name()),
                    )
                }
            }
        }
        // bare type variable: bind it (all bindings must agree)
        if params.iter().any(|p| p == base_full) {
            if let Some(prev) = binds.get(base_full) {
                if *prev != arg_inner {
                    return err_at(
                        sp,
                        format!(
                            "cannot infer type parameter '{base_full}': {} vs {} — cast explicitly",
                            prev.name(),
                            arg_inner.name()
                        ),
                    );
                }
                return Ok(());
            }
            binds.insert(base_full.to_string(), arg_inner);
            return Ok(());
        }
        // pattern instantiates another generic struct: Box[T] vs Box__int
        if let Some(i) = base_full.find('[') {
            let base = &base_full[..i];
            let inner = base_full[i..]
                .strip_prefix('[')
                .and_then(|r| r.strip_suffix(']'))
                .unwrap_or("");
            let pat_args = split_top_level_args(inner);
            let Type::Struct(mangled) = &arg_inner else {
                return err_at(
                    sp,
                    format!("generic argument: expected '{pattern}', got '{}'", arg_inner.name()),
                );
            };
            let origin_ok = self.struct_origin.get(mangled).map(|o| o == base).unwrap_or(false);
            let concrete_args = self.struct_args.get(mangled);
            match (origin_ok, concrete_args) {
                (true, Some(cargs)) if cargs.len() == pat_args.len() => {
                    for (pa, ca) in pat_args.iter().zip(cargs) {
                        self.unify_pattern(pa, ca, params, binds, sp)?;
                    }
                    Ok(())
                }
                _ => err_at(
                    sp,
                    format!("generic argument: expected '{pattern}', got '{}'", arg_inner.name()),
                ),
            }
        } else {
            // concrete pattern: types must match exactly, stars included
            // (an untyped literal adaptation is re-checked later by the
            // normal call path)
            if resolve_concrete(pattern) != *arg {
                return err_at(
                    sp,
                    format!("generic argument: expected '{pattern}', got '{}'", arg.name()),
                );
            }
            Ok(())
        }
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
        let base_full = &s[stars..];
        // generic arguments: "Box[int]" -> base "Box", args ["int"]
        let (base, gen_args) = match base_full.find('[') {
            Some(i) => (&base_full[..i], Some(&base_full[i..])),
            None => (base_full, None),
        };
        if let Some(args_s) = gen_args {
            let inner = args_s
                .strip_prefix('[')
                .and_then(|r| r.strip_suffix(']'))
                .ok_or_else(|| CheckError {
                    msg: format!("bad type '{s}'"),
                    span,
                })?;
            let args = split_top_level_args(inner);
            if Type::from_builtin(base).is_some() {
                return err_at(span, format!("type '{base}' takes no type parameters"));
            }
            if self.generic_structs.contains_key(base) {
                let mangled = self.instantiate_struct(base, args, span)?;
                let mut out = Type::Struct(mangled);
                for _ in 0..stars {
                    out = Type::Ptr(Box::new(out));
                }
                return Ok(out);
            }
            if self.generic_enums.contains_key(base) {
                let mangled = self.instantiate_enum(base, args, span)?;
                let mut out = Type::Enum(mangled);
                for _ in 0..stars {
                    out = Type::Ptr(Box::new(out));
                }
                return Ok(out);
            }
            if self.structs.contains_key(base) {
                return err_at(span, format!("struct '{base}' is not generic"));
            }
            if self.enums.contains_key(base) {
                return err_at(span, format!("enum '{base}' is not generic"));
            }
            return err_at(span, format!("unknown type '{base}'"));
        }
        let t = match Type::from_builtin(base) {
            Some(t) => t,
            None => {
                if self.structs.contains_key(base) {
                    self.used_structs.insert(base.to_string());
                    Type::Struct(base.to_string())
                } else if self.enums.contains_key(base) {
                    self.used_enums.insert(base.to_string());
                    Type::Enum(base.to_string())
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

    /// Types an expression and records the type on the node (`.ty`), so the
    /// codegen can emit mode-specific code (safe-mode bounds/div checks)
    /// without a second type pass.
    fn expr_ty(&mut self, e: &mut Expr) -> CResult<Type> {
        let t = self.expr_ty_inner(e)?;
        e.ty = Some(t.name());
        Ok(t)
    }

    /// Types an expression, mutating auto-deref into Field objects.
    fn expr_ty_inner(&mut self, e: &mut Expr) -> CResult<Type> {
        let sp = e.span;
        // enum variant construction (`Circle(3.0)`, `Ok(5)`) rewrites the
        // node, so it dispatches before the general call path
        if let ExprKind::Call(name, ty_args, _) = &e.node {
            if self.variant_owner.contains_key(name.as_str()) {
                if !ty_args.is_empty() {
                    return err_at(
                        sp,
                        format!(
                            "type arguments are not valid on the enum constructor '{name}'"
                        ),
                    );
                }
                return self.check_variant_construct(e, sp);
            }
        }
        match &mut e.node {
            ExprKind::Int(_) => Ok(Type::Int(IntW::I64)),
            ExprKind::UInt(_) => Ok(Type::Int(IntW::U64)),
            ExprKind::Float(_) => Ok(Type::Float(FloatW::F64)),
            ExprKind::Str(_) => Ok(Type::Str),
            ExprKind::Bool(_) => Ok(Type::Bool),
            ExprKind::Ident(n) => {
                // locals shadow variant names; a bare zero-payload variant
                // (`Point`, `None`) is a value
                if self.lookup(n).is_none() && self.variant_owner.contains_key(n) {
                    return self.check_variant_construct(e, sp);
                }
                self.lookup(n).ok_or_else(|| CheckError {
                    msg: format!("undefined variable '{n}'"),
                    span: sp,
                })
            }
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
                    Type::Enum(n) => {
                        return err_at(
                            sp,
                            format!("enum '{n}' has no fields — destructure it with match"),
                        )
                    }
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
                // generic struct literal: infer type arguments from the
                // field expressions, monomorphize, then re-check concretely
                if self.generic_structs.contains_key(name.as_str())
                    && !self.structs.contains_key(name.as_str())
                {
                    let gen = self.generic_structs.get(name.as_str()).unwrap().clone();
                    let mut ftypes = Vec::with_capacity(fields.len());
                    for (_, fe) in fields.iter_mut() {
                        ftypes.push(self.expr_ty(fe)?);
                    }
                    if fields.len() != gen.fields.len() {
                        return err_at(
                            sp,
                            format!(
                                "struct '{}': expected {} field(s), got {}",
                                name,
                                gen.fields.len(),
                                fields.len()
                            ),
                        );
                    }
                    let mut binds = HashMap::new();
                    for (i, (fname, _)) in fields.iter().enumerate() {
                        let pat = gen
                            .fields
                            .iter()
                            .find(|(n, _)| n == fname)
                            .map(|(_, t)| t.clone())
                            .ok_or_else(|| CheckError {
                                msg: format!("struct '{name}' has no field '{fname}'"),
                                span: sp,
                            })?;
                        self.unify_pattern(&pat, &ftypes[i], &gen.type_params, &mut binds, sp)?;
                    }
                    for tp in &gen.type_params {
                        if !binds.contains_key(tp) {
                            return err_at(
                                sp,
                                format!(
                                    "cannot infer type parameter '{tp}' of struct '{name}' — it does not appear in any provided field"
                                ),
                            );
                        }
                    }
                    let arg_strs: Vec<String> =
                        gen.type_params.iter().map(|tp| binds[tp].name()).collect();
                    let mangled = self.instantiate_struct(name.as_str(), arg_strs, sp)?;
                    let taken = std::mem::take(fields);
                    *e = Spanned::new(ExprKind::StructLit(mangled, taken), sp);
                    return self.expr_ty(e);
                }
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
                                ExprKind::Call("_wlel_strcat".into(), vec![], vec![left, right]),
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
                                    ExprKind::Call(
                                        "_wlel_streq".into(),
                                        vec![],
                                        vec![left, right],
                                    ),
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
                        } else if matches!(lt, Type::Enum(_)) {
                            return err_at(
                                sp,
                                format!(
                                    "cannot compare enum values with '{:?}' — destructure with match",
                                    op
                                ),
                            );
                        } else if matches!(lt, Type::Struct(_) | Type::Array(..)) {
                            return err_at(
                                sp,
                                format!(
                                    "cannot compare {} values with '{:?}' — compare field by field",
                                    lt.name(),
                                    op
                                ),
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
                    // strings are immutable C strings: indexing reads one
                    // byte (the NUL at s[len] is a legal read, so bounds
                    // can be tested with `s[i] != 0` loops)
                    Type::Str => Ok(Type::Int(IntW::U8)),
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
            ExprKind::EnumLit(ename, _tag, vname, payloads) => {
                // defensive: EnumLit nodes are produced by construction
                // rewrites that return the type directly; re-visiting still
                // type-checks cleanly (e.g. monomorphized body re-checks)
                let variants = self.enums.get(ename).cloned().ok_or_else(|| CheckError {
                    msg: format!("unknown enum '{ename}'"),
                    span: sp,
                })?;
                let vi = variants
                    .iter()
                    .position(|(n, _)| n == vname)
                    .ok_or_else(|| CheckError {
                        msg: format!("enum '{ename}' has no variant '{vname}'"),
                        span: sp,
                    })?;
                let payload = variants[vi].1.clone();
                for (p, pt) in payloads.iter_mut().zip(payload.iter()) {
                    let t = self.expr_ty(p)?;
                    if !assignable_checked(pt, p, &t, sp)? {
                        return err_at(
                            sp,
                            format!(
                                "enum '{ename}' variant '{vname}': payload expects {}, got {}",
                                pt.name(),
                                t.name()
                            ),
                        );
                    }
                }
                self.used_enums.insert(ename.clone());
                Ok(Type::Enum(ename.clone()))
            }
            ExprKind::Match(_) => err_at(
                sp,
                "match may only appear directly as a let/return value or as a statement",
            ),
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
            ExprKind::Call(name, ty_args, args) => {
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
                        // sys::mono_ms() — monotonic milliseconds (never
                        // jumps); the basis for timing/benchmark harnesses
                        "mono_ms" | "unix_ms" => {
                            if !args.is_empty() {
                                return err_at(
                                    sp,
                                    format!("sys::{rest}() takes no arguments"),
                                );
                            }
                            return Ok(Type::Int(IntW::I64));
                        }
                        // sys::read_file(path, &out) -> bool — the whole file
                        // as a NUL-terminated string allocated in the active
                        // arena; false on any failure (*out untouched)
                        "read_file" => {
                            if args.len() != 2 {
                                return err_at(
                                    sp,
                                    "sys::read_file(path, &out) takes exactly 2 arguments",
                                );
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            if t != Type::Str {
                                return err_at(
                                    sp,
                                    format!(
                                        "sys::read_file: path must be string, got {}",
                                        t.name()
                                    ),
                                );
                            }
                            let t = self.expr_ty(&mut args[1])?;
                            if t != Type::Ptr(Box::new(Type::Str)) {
                                return err_at(
                                    sp,
                                    format!(
                                        "sys::read_file: out must be *string, got {}",
                                        t.name()
                                    ),
                                );
                            }
                            let taken = std::mem::take(args);
                            *e = Spanned::new(
                                ExprKind::Call("_wlel_read_file".into(), vec![], taken),
                                sp,
                            );
                            return Ok(Type::Bool);
                        }
                        // sys::write_file(path, contents) -> bool — creates or
                        // truncates the file and writes the string
                        "write_file" => {
                            if args.len() != 2 {
                                return err_at(
                                    sp,
                                    "sys::write_file(path, contents) takes exactly 2 arguments",
                                );
                            }
                            let t = self.expr_ty(&mut args[0])?;
                            if t != Type::Str {
                                return err_at(
                                    sp,
                                    format!(
                                        "sys::write_file: path must be string, got {}",
                                        t.name()
                                    ),
                                );
                            }
                            let t = self.expr_ty(&mut args[1])?;
                            if t != Type::Str {
                                return err_at(
                                    sp,
                                    format!(
                                        "sys::write_file: contents must be string, got {}",
                                        t.name()
                                    ),
                                );
                            }
                            let taken = std::mem::take(args);
                            *e = Spanned::new(
                                ExprKind::Call("_wlel_write_file".into(), vec![], taken),
                                sp,
                            );
                            return Ok(Type::Bool);
                        }
                        other => {
                            return err_at(sp, format!("unknown sys function 'sys::{other}'"))
                        }
                    }
                }
                // std module
                if let Some(rest) = name.strip_prefix("std::") {
                    // the embedded library itself may always use std::
                    // (vec/string code calls strlen & checked ops), but its
                    // calls must not mark the USER's import as used
                    let in_std = self.current_file == STD_FILE;
                    if !self.use_std && !in_std {
                        return err_at(sp, format!("'{}' requires `use std;`", name));
                    }
                    if !in_std {
                        self.std_called = true;
                    }
                    // std::format(fmt, ...args) — mini-printf with `{}` slots.
                    // Variadic, so it cannot be a plain std function: the
                    // call is rewritten to a per-argument-kind C helper
                    // (`_wlel_format_iis` = int, int, string) that codegen
                    // emits only when used.
                    if rest == "format" {
                        if args.is_empty() {
                            return err_at(sp, "std::format takes a format string plus arguments");
                        }
                        let t = self.expr_ty(&mut args[0])?;
                        if t != Type::Str {
                            return err_at(
                                sp,
                                format!(
                                    "std::format: format must be string, got {}",
                                    t.name()
                                ),
                            );
                        }
                        let mut kinds = String::new();
                        for a in args.iter_mut().skip(1) {
                            let t = self.expr_ty(a)?;
                            match t {
                                Type::Int(_) => kinds.push('i'),
                                Type::Float(_) => kinds.push('f'),
                                Type::Bool => kinds.push('b'),
                                Type::Str => kinds.push('s'),
                                other => {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::format supports int, float, bool and string arguments, got {}",
                                            other.name()
                                        ),
                                    )
                                }
                            }
                        }
                        let sig = if kinds.is_empty() { "0".to_string() } else { kinds };
                        let taken = std::mem::take(args);
                        *e = Spanned::new(
                            ExprKind::Call(format!("_wlel_format_{sig}"), vec![], taken),
                            sp,
                        );
                        return Ok(Type::Str);
                    }
                    // std::fs::* — defer-friendly file handles. `File` is the
                    // built-in wrapper of a C FILE*; open returns 0 (null) on
                    // failure, read/write return byte counts (-1 on error),
                    // close is idempotent (safe to defer and to call again)
                    if let Some(fs_fn) = rest.strip_prefix("fs::") {
                        let file_ptr = Type::Ptr(Box::new(Type::Struct("File".into())));
                        match fs_fn {
                            "open" => {
                                if args.len() != 2 {
                                    return err_at(
                                        sp,
                                        "std::fs::open(path, mode) takes exactly 2 arguments",
                                    );
                                }
                                let t = self.expr_ty(&mut args[0])?;
                                if t != Type::Str {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::fs::open: path must be string, got {}",
                                            t.name()
                                        ),
                                    );
                                }
                                let t = self.expr_ty(&mut args[1])?;
                                if t != Type::Str {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::fs::open: mode must be string (\"r\", \"w\", \"a\", ...), got {}",
                                            t.name()
                                        ),
                                    );
                                }
                                let taken = std::mem::take(args);
                                *e = Spanned::new(
                                    ExprKind::Call("_wlel_fs_open".into(), vec![], taken),
                                    sp,
                                );
                                return Ok(file_ptr);
                            }
                            "read" | "write" => {
                                if args.len() != 3 {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::fs::{fs_fn}(f, buf, n) takes exactly 3 arguments"
                                        ),
                                    );
                                }
                                let t = self.expr_ty(&mut args[0])?;
                                if t != file_ptr {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::fs::{fs_fn}: f must be *File, got {}",
                                            t.name()
                                        ),
                                    );
                                }
                                let t = self.expr_ty(&mut args[1])?;
                                if t != Type::Ptr(Box::new(Type::Int(IntW::U8))) {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::fs::{fs_fn}: buf must be *u8 (cast explicitly), got {}",
                                            t.name()
                                        ),
                                    );
                                }
                                let t = self.expr_ty(&mut args[2])?;
                                if !is_int(&t) {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::fs::{fs_fn}: n must be int, got {}",
                                            t.name()
                                        ),
                                    );
                                }
                                let helper = if fs_fn == "read" {
                                    "_wlel_fs_read"
                                } else {
                                    "_wlel_fs_write"
                                };
                                let taken = std::mem::take(args);
                                *e = Spanned::new(
                                    ExprKind::Call(helper.into(), vec![], taken),
                                    sp,
                                );
                                return Ok(Type::Int(IntW::I64));
                            }
                            "close" => {
                                if args.len() != 1 {
                                    return err_at(sp, "std::fs::close(f) takes exactly 1 argument");
                                }
                                let t = self.expr_ty(&mut args[0])?;
                                if t != file_ptr {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::fs::close: f must be *File, got {}",
                                            t.name()
                                        ),
                                    );
                                }
                                let taken = std::mem::take(args);
                                *e = Spanned::new(
                                    ExprKind::Call("_wlel_fs_close".into(), vec![], taken),
                                    sp,
                                );
                                return Ok(Type::Void);
                            }
                            other => {
                                return err_at(
                                    sp,
                                    format!("unknown std function 'std::fs::{other}'"),
                                )
                            }
                        }
                    }
                    // std::math::* — thin wrappers over C99 libm: float in,
                    // float out. Integers adapt to float per the usual
                    // literal/widening rules; other types need an explicit
                    // cast. f32 arguments must cast explicitly (mixed-width
                    // rule), matching the rest of the language.
                    if let Some(mfn) = rest.strip_prefix("math::") {
                        const MATH1: &[&str] = &[
                            "sqrt", "cbrt", "exp", "log", "log2", "log10", "sin", "cos", "tan",
                            "asin", "acos", "atan", "sinh", "cosh", "tanh", "floor", "ceil",
                            "round", "trunc", "fabs",
                        ];
                        const MATH2: &[&str] = &[
                            "pow", "atan2", "fmin", "fmax", "hypot", "fmod",
                        ];
                        let want = if MATH1.contains(&mfn) {
                            1
                        } else if MATH2.contains(&mfn) {
                            2
                        } else {
                            return err_at(
                                sp,
                                format!("unknown std function 'std::math::{mfn}'"),
                            );
                        };
                        if args.len() != want {
                            return err_at(
                                sp,
                                format!(
                                    "std::math::{mfn} takes exactly {want} argument(s), got {}",
                                    args.len()
                                ),
                            );
                        }
                        let f64t = Type::Float(FloatW::F64);
                        for a in args.iter_mut() {
                            let t = self.expr_ty(a)?;
                            if !assignable_checked(&f64t, a, &t, sp)? {
                                return err_at(
                                    sp,
                                    format!(
                                        "std::math::{mfn}: argument must be float (f64), got {}",
                                        t.name()
                                    ),
                                );
                            }
                        }
                        return Ok(f64t);
                    }
                    // std::random::* — xorshift64* PRNG. Auto-seeded from the
                    // monotonic clock (+ address entropy) on first use;
                    // seed(s) pins the stream for reproducible runs.
                    if let Some(rfn) = rest.strip_prefix("random::") {
                        match rfn {
                            "seed" => {
                                if args.len() != 1 {
                                    return err_at(
                                        sp,
                                        "std::random::seed(s) takes exactly 1 argument",
                                    );
                                }
                                let t = self.expr_ty(&mut args[0])?;
                                if !is_int(&t) {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::random::seed: s must be int, got {}",
                                            t.name()
                                        ),
                                    );
                                }
                                return Ok(Type::Void);
                            }
                            "next" => {
                                if !args.is_empty() {
                                    return err_at(
                                        sp,
                                        "std::random::next() takes no arguments",
                                    );
                                }
                                return Ok(Type::Int(IntW::U64));
                            }
                            "int" => {
                                if args.len() != 1 {
                                    return err_at(
                                        sp,
                                        "std::random::int(n) takes exactly 1 argument",
                                    );
                                }
                                let t = self.expr_ty(&mut args[0])?;
                                if !is_int(&t) {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::random::int: n must be int, got {}",
                                            t.name()
                                        ),
                                    );
                                }
                                return Ok(Type::Int(IntW::I64));
                            }
                            "float" => {
                                if !args.is_empty() {
                                    return err_at(
                                        sp,
                                        "std::random::float() takes no arguments",
                                    );
                                }
                                return Ok(Type::Float(FloatW::F64));
                            }
                            other => {
                                return err_at(
                                    sp,
                                    format!("unknown std function 'std::random::{other}'"),
                                )
                            }
                        }
                    }
                    // std::sort / std::binary_search — in-place sort and
                    // sorted lookup driven by a user comparator
                    // `cmp(a, b) -> int` (negative / zero / positive, like
                    // strcmp). Accepted collections:
                    //   std::sort(arr, cmp)        — fixed array [T; N]
                    //   std::sort(base, n, cmp)    — *T pointer + count
                    //   std::sort(v, cmp)          — Vec-like struct
                    //                                (data/len fields)
                    // binary_search takes the needle before the comparator
                    // in every form and returns a matching index or -1.
                    // Wlel has no function pointers: the comparator is
                    // resolved by name and codegen emits a specialized C
                    // helper per (element, comparator) pair that calls it
                    // directly, so the optimizer can inline it.
                    if rest == "sort" || rest == "binary_search" {
                        let searching = rest == "binary_search";
                        if args.len() < 2 {
                            return err_at(
                                sp,
                                if searching {
                                    "std::binary_search takes (arr, needle, cmp), (ptr, n, needle, cmp) or (vec, needle, cmp)"
                                } else {
                                    "std::sort takes (arr, cmp), (ptr, n, cmp) or (vec, cmp)"
                                },
                            );
                        }
                        let nvals = args.len() - 1;
                        let mut val_tys = Vec::with_capacity(nvals);
                        for a in args[..nvals].iter_mut() {
                            val_tys.push(self.expr_ty(a)?);
                        }
                        // the comparator must name a concrete function
                        let cmp_span = args[nvals].span;
                        let cmp_name = match &args[nvals].node {
                            ExprKind::Ident(n) => n.clone(),
                            _ => {
                                return err_at(
                                    cmp_span,
                                    format!(
                                        "std::{rest}: comparator must be a function name, e.g. by_asc"
                                    ),
                                )
                            }
                        };
                        let cmp_sig = match self.sigs.get(&cmp_name).cloned() {
                            Some(s) => s,
                            None => {
                                if self.generic_funcs.contains_key(&cmp_name) {
                                    return err_at(
                                        cmp_span,
                                        format!(
                                            "std::{rest}: comparator '{cmp_name}' is generic — instantiate it or wrap it in a concrete function"
                                        ),
                                    );
                                }
                                return err_at(
                                    cmp_span,
                                    format!("std::{rest}: unknown comparator function '{cmp_name}'"),
                                );
                            }
                        };
                        if !matches!(cmp_sig.ret, Type::Int(IntW::I64)) {
                            return err_at(
                                cmp_span,
                                format!(
                                    "std::{rest}: comparator '{cmp_name}' must return int, got {}",
                                    cmp_sig.ret.name()
                                ),
                            );
                        }
                        self.called_funcs.insert(cmp_name.clone());
                        // resolve the collection into (base, len, elem);
                        // every form validates its own argument count
                        let base: Expr;
                        let len: Expr;
                        let mut needle: Option<Expr> = None;
                        let mut needle_ty = Type::Void;
                        let elem: Type;
                        let array_msg = |searching: bool| {
                            if searching {
                                "std::binary_search: an array carries its length — pass (arr, needle, cmp)"
                            } else {
                                "std::sort: an array carries its length — pass (arr, cmp)"
                            }
                        };
                        match &val_tys[0] {
                            Type::Array(el, n) => {
                                if nvals != if searching { 2 } else { 1 } {
                                    return err_at(sp, array_msg(searching));
                                }
                                elem = (**el).clone();
                                base = std::mem::replace(&mut args[0], dummy_expr(sp));
                                len = Spanned::new(ExprKind::Int(*n as i64), sp);
                                if searching {
                                    needle_ty = val_tys[1].clone();
                                    needle =
                                        Some(std::mem::replace(&mut args[1], dummy_expr(sp)));
                                }
                            }
                            Type::Struct(_) => {
                                if nvals != if searching { 2 } else { 1 } {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::{rest}: a Vec-like value carries its length — pass the value{} plus the comparator",
                                            if searching { ", the needle" } else { "" }
                                        ),
                                    );
                                }
                                let sname = match &val_tys[0] {
                                    Type::Struct(n) => n.clone(),
                                    _ => unreachable!(),
                                };
                                let fields = self.structs.get(&sname).cloned();
                                let data_el = fields.as_ref().and_then(|fs| {
                                    fs.iter()
                                        .find(|(n, _)| n == "data")
                                        .and_then(|(_, t)| match t {
                                            Type::Ptr(el) => Some((**el).clone()),
                                            _ => None,
                                        })
                                });
                                let has_len = fields
                                    .as_ref()
                                    .and_then(|fs| {
                                        fs.iter()
                                            .find(|(n, _)| n == "len")
                                            .map(|(_, t)| is_int(t))
                                    })
                                    .unwrap_or(false);
                                match (data_el, has_len) {
                                    (Some(el), true) => {
                                        let coll =
                                            std::mem::replace(&mut args[0], dummy_expr(sp));
                                        base = Spanned::new(
                                            ExprKind::Field(
                                                Box::new(coll.clone()),
                                                "data".into(),
                                            ),
                                            sp,
                                        );
                                        len = Spanned::new(
                                            ExprKind::Field(Box::new(coll), "len".into()),
                                            sp,
                                        );
                                        elem = el;
                                    }
                                    _ => {
                                        return err_at(
                                            sp,
                                            format!(
                                                "std::{rest}: '{}' has no data/len element buffer — pass an array or a (pointer, length) pair",
                                                val_tys[0].name()
                                            ),
                                        );
                                    }
                                }
                                if searching {
                                    needle_ty = val_tys[1].clone();
                                    needle =
                                        Some(std::mem::replace(&mut args[1], dummy_expr(sp)));
                                }
                            }
                            Type::Ptr(el) => {
                                if nvals != if searching { 3 } else { 2 } {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::{rest}: a pointer needs an explicit length — pass (pointer, n{} comparator)",
                                            if searching { ", needle," } else { "," }
                                        ),
                                    );
                                }
                                if !is_int(&val_tys[1]) {
                                    return err_at(
                                        sp,
                                        format!(
                                            "std::{rest}: n must be int, got {}",
                                            val_tys[1].name()
                                        ),
                                    );
                                }
                                elem = (**el).clone();
                                base = std::mem::replace(&mut args[0], dummy_expr(sp));
                                len = std::mem::replace(&mut args[1], dummy_expr(sp));
                                if searching {
                                    needle_ty = val_tys[2].clone();
                                    needle =
                                        Some(std::mem::replace(&mut args[2], dummy_expr(sp)));
                                }
                            }
                            other => {
                                return err_at(
                                    sp,
                                    format!(
                                        "std::{rest}: cannot {} a value of type '{}' — pass an array, a (pointer, length) pair or a Vec-like struct",
                                        if searching { "search" } else { "sort" },
                                        other.name()
                                    ),
                                );
                            }
                        }
                        if matches!(elem, Type::Array(..)) {
                            return err_at(
                                sp,
                                format!(
                                    "std::{rest}: element type '{}' is not sortable — arrays of arrays are not supported",
                                    elem.name()
                                ),
                            );
                        }
                        if cmp_sig.params.len() != 2
                            || cmp_sig.params[0] != elem
                            || cmp_sig.params[1] != elem
                        {
                            let got = if cmp_sig.params.len() == 2 {
                                format!(
                                    "({}, {})",
                                    cmp_sig.params[0].name(),
                                    cmp_sig.params[1].name()
                                )
                            } else {
                                format!("{} parameter(s)", cmp_sig.params.len())
                            };
                            return err_at(
                                cmp_span,
                                format!(
                                    "std::{rest}: comparator '{cmp_name}' must take two '{}' arguments, got {got}",
                                    elem.name()
                                ),
                            );
                        }
                        if let Some(nd) = &needle {
                            if !assignable_checked(&elem, nd, &needle_ty, sp)? {
                                return err_at(
                                    sp,
                                    format!(
                                        "std::binary_search: needle must be {}, got {}",
                                        elem.name(),
                                        needle_ty.name()
                                    ),
                                );
                            }
                        }
                        let helper = if searching { "_wlel_bsearch" } else { "_wlel_sort" };
                        let mut new_args = vec![base, len];
                        if let Some(nd) = needle {
                            new_args.push(nd);
                        }
                        new_args.push(Spanned::new(ExprKind::Str(elem.name()), sp));
                        new_args.push(Spanned::new(ExprKind::Str(cmp_name), sp));
                        *e = Spanned::new(
                            ExprKind::Call(helper.into(), vec![], new_args),
                            sp,
                        );
                        return Ok(if searching {
                            Type::Int(IntW::I64)
                        } else {
                            Type::Void
                        });
                    }
                    let want_args: &[Type] = match rest {
                        "print_int" | "println_int" => &[Type::Int(IntW::I64)],
                        "print_float" | "println_float" => &[Type::Float(FloatW::F64)],
                        "print_str" | "println_str" => &[Type::Str],
                        "strlen" => &[Type::Str],
                        "streq" => &[Type::Str, Type::Str],
                        "float_to_str" => &[Type::Float(FloatW::F64)],
                        // parse_float(s, &out): strtod semantics, but the
                        // whole string must be consumed; false leaves *out
                        // untouched
                        "parse_float" => &[
                            Type::Str,
                            Type::Ptr(Box::new(Type::Float(FloatW::F64))),
                        ],
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
                        "parse_float" => Type::Bool,
                        "strlen" => Type::Int(IntW::I64),
                        "float_to_str" => Type::Str,
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
                // assert(cond) — builtin: rewrites to _wlel_assert(cond, file, line)
                // so failures report file:line at runtime (works in tests and
                // ordinary code; exits outside a test run)
                if name == "assert" {
                    if args.len() != 1 {
                        return err_at(sp, "assert() takes exactly 1 argument");
                    }
                    let t = self.expr_ty(&mut args[0])?;
                    if t != Type::Bool {
                        return err_at(
                            sp,
                            format!("assert() expects a bool condition, got {}", t.name()),
                        );
                    }
                    let cond = std::mem::replace(&mut args[0], dummy_expr(sp));
                    *e = self.wlel_assert_call(cond, sp);
                    return Ok(Type::Void);
                }
                // assert_eq(a, b) — builtin: same comparison semantics as `==`
                // (literal adaptation, width rules), strings compare by content
                if name == "assert_eq" {
                    if args.len() != 2 {
                        return err_at(sp, "assert_eq() takes exactly 2 arguments");
                    }
                    let (left_args, right_args) = args.split_at_mut(1);
                    let eq = self.build_assert_eq(&mut left_args[0], &mut right_args[0], sp)?;
                    *e = self.wlel_assert_call(eq, sp);
                    return Ok(Type::Void);
                }
                // arena_stats() — builtin: statistics of the active arena
                // (root when no arena block is open) as a built-in struct
                if name == "arena_stats" {
                    if !args.is_empty() {
                        return err_at(sp, "arena_stats() takes no arguments");
                    }
                    return Ok(Type::Struct("ArenaStats".into()));
                }
                // _wlel_hash(x) — internal hashing primitive used by
                // HashMap (prefix `_wlel_` is reserved, so user code and
                // even the std source treat it as a compiler builtin)
                if name == "_wlel_hash" {
                    if args.len() != 1 || !ty_args.is_empty() {
                        return err_at(sp, "_wlel_hash takes exactly 1 argument");
                    }
                    let t = self.expr_ty(&mut args[0])?;
                    let helper = match &t {
                        Type::Int(_) | Type::Bool => "_wlel_hash_i64",
                        Type::Float(_) => "_wlel_hash_f64",
                        Type::Str => "_wlel_hash_str",
                        Type::Ptr(_) => "_wlel_hash_ptr",
                        other => {
                            return err_at(
                                sp,
                                format!(
                                    "hash key must be int, float, bool, string or pointer, got {}",
                                    other.name()
                                ),
                            )
                        }
                    };
                    let taken = std::mem::take(args);
                    *e = Spanned::new(ExprKind::Call(helper.into(), vec![], taken), sp);
                    return Ok(Type::Int(IntW::U64));
                }
                // generic function call. With explicit type arguments
                // (`vec_new[int]()`) the bindings come straight from the
                // source; otherwise they are inferred from argument types.
                // Either way the call is monomorphized and re-checked as a
                // normal call to the mangled name.
                if !ty_args.is_empty() && self.generic_funcs.contains_key(name.as_str()) {
                    let gen = self.generic_funcs.get(name.as_str()).unwrap().clone();
                    if ty_args.len() != gen.type_params.len() {
                        return err_at(
                            sp,
                            format!(
                                "'{}' takes {} type argument(s), got {}",
                                name,
                                gen.type_params.len(),
                                ty_args.len()
                            ),
                        );
                    }
                    let mut binds = HashMap::new();
                    for (tp, ta) in gen.type_params.iter().zip(ty_args.iter()) {
                        binds.insert(tp.clone(), self.resolve_type_str(ta, sp)?);
                    }
                    self.called_funcs.insert(name.clone());
                    let mangled = self.instantiate_fn(name.as_str(), &binds, sp)?;
                    let taken = std::mem::take(args);
                    *e = Spanned::new(ExprKind::Call(mangled, vec![], taken), sp);
                    return self.expr_ty(e);
                }
                if ty_args.is_empty()
                    && self.generic_funcs.contains_key(name.as_str())
                    && !self.sigs.contains_key(name.as_str())
                {
                    let gen = self.generic_funcs.get(name.as_str()).unwrap().clone();
                    if args.len() != gen.params.len() {
                        return err_at(
                            sp,
                            format!(
                                "'{}' takes {} argument(s), got {}",
                                name,
                                gen.params.len(),
                                args.len()
                            ),
                        );
                    }
                    let mut arg_types = Vec::with_capacity(args.len());
                    for a in args.iter_mut() {
                        arg_types.push(self.expr_ty(a)?);
                    }
                    let mut binds = HashMap::new();
                    for (p, at) in gen.params.iter().zip(arg_types.iter()) {
                        let pat = p.ty.as_deref().unwrap_or("int");
                        self.unify_pattern(pat, at, &gen.type_params, &mut binds, sp)?;
                    }
                    for tp in &gen.type_params {
                        if !binds.contains_key(tp) {
                            return err_at(
                                sp,
                                format!(
                                    "cannot infer type parameter '{tp}' of '{name}' — it does not appear in any parameter type"
                                ),
                            );
                        }
                    }
                    self.called_funcs.insert(name.clone());
                    let mangled = self.instantiate_fn(name.as_str(), &binds, sp)?;
                    let taken = std::mem::take(args);
                    *e = Spanned::new(ExprKind::Call(mangled, vec![], taken), sp);
                    return self.expr_ty(e);
                }
                if !ty_args.is_empty() {
                    return err_at(
                        sp,
                        format!("function '{name}' is not generic — remove the [T] list"),
                    );
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
                // seed the expected-type hint when a generic enum variant is
                // constructed directly into an annotated variable, so
                // `let r: Result[int, string] = Ok(5);` knows its `E`
                if let Some(hint) = self.let_enum_hint(ty_ann.as_deref(), init, sp) {
                    self.expected = Some(hint);
                }
                let init_ty = match &mut init.node {
                    ExprKind::Match(m) => {
                        let t = self.check_match(m, true, init.span)?;
                        init.ty = Some(t.name());
                        t
                    }
                    _ => self.expr_ty(init)?,
                };
                self.expected = None;
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
                // s[i] on a string is a read-only byte view: strings never
                // mutate in place, new strings come from + / str_sub
                if let ExprKind::Index(base, _) = &a.target.node {
                    if base.ty.as_deref() == Some("string") {
                        return err_at(
                            sp,
                            "strings are immutable — build a new one with + or str_sub",
                        );
                    }
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
            StmtKind::ForIn(f) => {
                let it_ty = self.expr_ty(&mut f.iter)?;
                let sp = s.span;
                // element type: arrays expose it directly; a Vec-shaped
                // struct exposes it through its `data: *T` field
                let elem: Type = match &it_ty {
                    Type::Array(el, _) => (**el).clone(),
                    Type::Struct(sname) => {
                        let data_ty = self
                            .structs
                            .get(sname)
                            .and_then(|fs| fs.iter().find(|(n, _)| n == "data"))
                            .map(|(_, t)| t.clone());
                        match data_ty {
                            Some(Type::Ptr(el)) => *el,
                            _ => {
                                return err_at(
                                    sp,
                                    format!(
                                        "cannot iterate over {} — for-in supports arrays and Vec[T]",
                                        it_ty.name()
                                    ),
                                )
                            }
                        }
                    }
                    other => {
                        return err_at(
                            sp,
                            format!(
                                "cannot iterate over {} — for-in supports arrays and Vec[T]",
                                other.name()
                            ),
                        )
                    }
                };
                if elem == Type::Void {
                    return err_at(sp, "cannot iterate over void elements");
                }
                f.elem = Some(elem.name());
                self.loop_depth += 1;
                self.scopes.push(HashMap::new());
                let outer_terminated =
                    std::mem::replace(&mut self.block_terminated, false);
                self.declare("for", &f.var, elem, sp)?;
                for st in f.body.0.iter_mut() {
                    if self.block_terminated {
                        self.warnings.push(CheckWarning {
                            msg: "unreachable statement".into(),
                            span: st.span,
                        });
                    }
                    self.check_stmt(st)?;
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
                    Some(e) => {
                        // seed the expected-type hint from the function's
                        // return type for a direct generic-variant return
                        if let Some(hint) = self.return_enum_hint(e) {
                            self.expected = Some(hint);
                        }
                        let t = match &mut e.node {
                            ExprKind::Match(m) => {
                                let t = self.check_match(m, true, e.span)?;
                                e.ty = Some(t.name());
                                t
                            }
                            _ => self.expr_ty(e)?,
                        };
                        self.expected = None;
                        t
                    }
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
            StmtKind::Match(ms) => {
                self.check_match(ms, false, sp)?;
                self.block_terminated = ms.all_arms_terminate;
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

    /// Build an enum value from a call to a variant name. Concrete enums
    /// construct directly; generic enums infer type arguments from the
    /// payload expressions (like generic struct literals), falling back to
    /// the expected-type hint seeded by an annotated `let`/`return`.
    fn check_variant_construct(&mut self, e: &mut Expr, sp: Span) -> CResult<Type> {
        // a call `Circle(x)` or a bare zero-payload value `Point`/`None`
        let (variant, is_call) = match &e.node {
            ExprKind::Call(name, _, _) => (name.clone(), true),
            ExprKind::Ident(name) => (name.clone(), false),
            _ => unreachable!("check_variant_construct on a non-call"),
        };
        let mut args: Vec<Expr> = match &mut e.node {
            ExprKind::Call(_, _, a) => std::mem::take(a),
            ExprKind::Ident(_) => Vec::new(),
            _ => unreachable!("check_variant_construct on a non-call"),
        };
        let owner = self.variant_owner.get(&variant).cloned().unwrap();
        if self.generic_enums.contains_key(&owner) {
            // infer bindings from the payload argument types
            let gen = self
                .generic_enums
                .get(&owner)
                .unwrap()
                .clone();
            let vdef = gen
                .variants
                .iter()
                .find(|v| v.name == variant)
                .ok_or_else(|| CheckError {
                    msg: format!("enum '{owner}' has no variant '{variant}'"),
                    span: sp,
                })?
                .clone();
            if args.len() != vdef.payloads.len() {
                return err_at(
                    sp,
                    format!(
                        "variant '{variant}' takes {} argument(s), got {}{}",
                        vdef.payloads.len(),
                        args.len(),
                        usage_hint(&variant, vdef.payloads.len(), is_call)
                    ),
                );
            }
            let mut arg_types = Vec::with_capacity(args.len());
            for a in args.iter_mut() {
                arg_types.push(self.expr_ty(a)?);
            }
            let mut binds: HashMap<String, Type> = HashMap::new();
            for (pat, at) in vdef.payloads.iter().zip(arg_types.iter()) {
                self.unify_pattern(pat, at, &gen.type_params, &mut binds, sp)?;
            }
            // uninferrable parameters may come from the expected-type hint
            if let Some((howner, hargs)) = &self.expected {
                if *howner == owner {
                    for (i, tp) in gen.type_params.iter().enumerate() {
                        if !binds.contains_key(tp) {
                            if let Some(ht) = hargs.get(i) {
                                binds.insert(tp.clone(), ht.clone());
                            }
                        }
                    }
                }
            }
            for tp in &gen.type_params {
                if !binds.contains_key(tp) {
                    return err_at(
                        sp,
                        format!(
                            "cannot infer type parameter '{tp}' of enum '{owner}' — annotate the variable: let r: {owner}[...] = {variant}(...)"
                        ),
                    );
                }
            }
            let arg_strs: Vec<String> =
                gen.type_params.iter().map(|tp| binds[tp].name()).collect();
            let mangled = self.instantiate_enum(&owner, arg_strs, sp)?;
            self.used_enums.insert(mangled.clone());
            // re-check the payloads against the concrete payload types
            let variants = self.enums.get(&mangled).cloned().ok_or_else(|| CheckError {
                msg: format!("unknown enum '{mangled}'"),
                span: sp,
            })?;
            let vi = variants
                .iter()
                .position(|(n, _)| n == &variant)
                .ok_or_else(|| CheckError {
                    msg: format!("enum '{mangled}' has no variant '{variant}'"),
                    span: sp,
                })?;
            for (i, a) in args.iter_mut().enumerate() {
                let want = variants[vi].1[i].clone();
                if !assignable_checked(&want, a, &arg_types[i], sp)? {
                    return err_at(
                        sp,
                        format!(
                            "variant '{variant}': payload {} expects {}, got {}",
                            i + 1,
                            want.name(),
                            arg_types[i].name()
                        ),
                    );
                }
            }
            *e = Spanned::new(
                ExprKind::EnumLit(mangled.clone(), vi, variant, args),
                sp,
            );
            return Ok(Type::Enum(mangled));
        }
        // concrete enum
        let variants = self.enums.get(&owner).cloned().ok_or_else(|| CheckError {
            msg: format!("unknown enum '{owner}'"),
            span: sp,
        })?;
        let vi = variants
            .iter()
            .position(|(n, _)| n == &variant)
            .ok_or_else(|| CheckError {
                msg: format!("enum '{owner}' has no variant '{variant}'"),
                span: sp,
            })?;
        let payload = variants[vi].1.clone();
        if args.len() != payload.len() {
            return err_at(
                sp,
                format!(
                    "variant '{variant}' takes {} argument(s), got {}{}",
                    payload.len(),
                    args.len(),
                    usage_hint(&variant, payload.len(), is_call)
                ),
            );
        }
        for (i, a) in args.iter_mut().enumerate() {
            let want = payload[i].clone();
            let got = self.expr_ty(a)?;
            if !assignable_checked(&want, a, &got, sp)? {
                return err_at(
                    sp,
                    format!(
                        "variant '{variant}': payload {} expects {}, got {}",
                        i + 1,
                        want.name(),
                        got.name()
                    ),
                );
            }
        }
        self.used_enums.insert(owner.clone());
        *e = Spanned::new(
            ExprKind::EnumLit(owner.clone(), vi, variant, args),
            sp,
        );
        Ok(Type::Enum(owner))
    }

    /// Type-check a `match`: the scrutinee must be an enum, every pattern
    /// must name one of its variants with the right payload arity, and the
    /// arms must cover every variant (or end in a `_` wildcard). In value
    /// form all arm expressions share one type.
    fn check_match(&mut self, m: &mut MatchStmt, value_form: bool, sp: Span) -> CResult<Type> {
        let scrut = self.expr_ty(&mut m.scrutinee)?;
        let enum_name = match scrut {
            Type::Enum(n) => n,
            other => {
                return err_at(
                    sp,
                    format!("match expects an enum value, got {}", other.name()),
                )
            }
        };
        self.used_enums.insert(enum_name.clone());
        let variants = self.enums.get(&enum_name).cloned().ok_or_else(|| CheckError {
            msg: format!("unknown enum '{enum_name}'"),
            span: sp,
        })?;
        let mut covered = vec![false; variants.len()];
        let mut wildcard = false;
        let mut result_ty: Option<Type> = None;
        let mut all_terminate = true;
        for (ai, arm) in m.arms.iter_mut().enumerate() {
            if wildcard {
                return err_at(arm.span, "arm after '_' wildcard is unreachable");
            }
            let mut arm_terminates = false;
            let mut bound = false;
            match &mut arm.pattern {
                Pattern::Wildcard => {
                    wildcard = true;
                }
                Pattern::Variant { name: vname, tag, binds } => {
                    let vname = vname.clone();
                    let vi = variants
                        .iter()
                        .position(|(n, _)| n == &vname)
                        .ok_or_else(|| CheckError {
                            msg: format!("enum '{enum_name}' has no variant '{vname}'"),
                            span: arm.span,
                        })?;
                    *tag = vi;
                    if covered[vi] {
                        return err_at(
                            arm.span,
                            format!("duplicate arm for variant '{vname}'"),
                        );
                    }
                    covered[vi] = true;
                    let payload = variants[vi].1.clone();
                    if binds.len() != payload.len() {
                        return err_at(
                            arm.span,
                            format!(
                                "variant '{vname}' carries {} payload value(s), pattern binds {}",
                                payload.len(),
                                binds.len()
                            ),
                        );
                    }
                    // bindings live only inside this arm; `_` is a discard
                    self.scopes.push(HashMap::new());
                    bound = true;
                    for (b, pt) in binds.iter_mut().zip(payload.iter()) {
                        b.ty = Some(pt.name());
                        if b.name == "_" {
                            continue;
                        }
                        self.declare("match", &b.name, pt.clone(), arm.span)?;
                    }
                }
            }
            // the arm body is checked for wildcard arms too — builtins such
            // as assert_eq are rewritten here and undefined names/types are
            // rejected; a wildcard arm is not a blanket "skip the checker"
            match &mut arm.body {
                MatchBody::Block(b) => {
                    if value_form {
                        if bound {
                            self.pop_scope();
                        }
                        return err_at(
                            arm.span,
                            "match arms in value position must be expressions",
                        );
                    }
                    self.check_block_mut(b)?;
                    arm_terminates = guarantees_return(b);
                }
                MatchBody::Expr(x) => {
                    let body_ty = self.expr_ty(x)?;
                    if value_form {
                        match &result_ty {
                            None => {
                                if body_ty == Type::Void {
                                    if bound {
                                        self.pop_scope();
                                    }
                                    return err_at(
                                        arm.span,
                                        "match arms cannot produce void — use a statement-form match",
                                    );
                                }
                                if matches!(body_ty, Type::Array(..)) {
                                    if bound {
                                        self.pop_scope();
                                    }
                                    return err_at(
                                        arm.span,
                                        format!(
                                            "match result type {} (an array) cannot be assigned — wrap it in a struct",
                                            body_ty.name()
                                        ),
                                    );
                                }
                                result_ty = Some(body_ty);
                            }
                            Some(want) => {
                                let ok = match &mut arm.body {
                                    MatchBody::Expr(x) => {
                                        assignable_checked(want, x, &body_ty, arm.span)?
                                    }
                                    MatchBody::Block(_) => false,
                                };
                                if !ok {
                                    if bound {
                                        self.pop_scope();
                                    }
                                    return err_at(
                                        arm.span,
                                        format!(
                                            "match arm {}: expected {}, got {}",
                                            ai + 1,
                                            want.name(),
                                            body_ty.name()
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if bound {
                self.pop_scope();
            }
            if !arm_terminates {
                all_terminate = false;
            }
        }
        let full_cover = wildcard || covered.iter().all(|c| *c);
        if !full_cover {
            let missing: Vec<&str> = variants
                .iter()
                .enumerate()
                .filter(|(i, _)| !covered[*i])
                .map(|(_, (n, _))| n.as_str())
                .collect();
            if !missing.is_empty() {
                return err_at(
                    sp,
                    format!(
                        "match on '{}' is not exhaustive: missing variant(s) {}",
                        enum_name,
                        missing.join(", ")
                    ),
                );
            }
        }
        m.all_arms_terminate = !value_form && full_cover && all_terminate && !m.arms.is_empty();
        if value_form {
            let t = result_ty.unwrap_or(Type::Void);
            m.result_ty = Some(t.name());
            Ok(t)
        } else {
            Ok(Type::Void)
        }
    }

    /// expected-type hint for a direct generic-variant construction as the
    /// initializer of an annotated let: `let r: Result[int, string] = Ok(...)`
    /// fills the `E` slot from the annotation
    fn let_enum_hint(&mut self, ann: Option<&str>, init: &Expr, sp: Span) -> Option<(String, Vec<Type>)> {
        let cname = match &init.node {
            ExprKind::Call(n, ta, _) if ta.is_empty() => n.clone(),
            ExprKind::Ident(n) => n.clone(),
            _ => return None,
        };
        let owner = self.variant_owner.get(&cname)?.clone();
        if !self.generic_enums.contains_key(&owner) {
            return None;
        }
        let ann = ann?;
        match self.resolve_type_str(ann, sp) {
            Ok(Type::Enum(m)) => {
                if self.enum_origin.get(&m).map(|o| o.as_str()) != Some(owner.as_str()) {
                    return None;
                }
                let args = self.enum_args.get(&m)?.clone();
                Some((owner, args))
            }
            _ => None,
        }
    }

    /// expected-type hint for a direct generic-variant construction as a
    /// return expression: `fn f() -> Result[int, string] { return Ok(5); }`
    fn return_enum_hint(&self, e: &Expr) -> Option<(String, Vec<Type>)> {
        let cname = match &e.node {
            ExprKind::Call(n, ta, _) if ta.is_empty() => n.clone(),
            ExprKind::Ident(n) => n.clone(),
            _ => return None,
        };
        let owner = self.variant_owner.get(&cname)?.clone();
        if !self.generic_enums.contains_key(&owner) {
            return None;
        }
        let Type::Enum(m) = &self.current_ret else {
            return None;
        };
        if self.enum_origin.get(m).map(|o| o.as_str()) != Some(owner.as_str()) {
            return None;
        }
        let args = self.enum_args.get(m)?.clone();
        Some((owner, args))
    }

    /// `_wlel_assert(cond, "file", line)` — the runtime aborts the active
    /// test (longjmp to the runner) or exits when no harness is running
    fn wlel_assert_call(&self, cond: Expr, sp: Span) -> Expr {
        Spanned::new(
            ExprKind::Call(
                "_wlel_assert".into(),
                vec![],
                vec![
                    cond,
                    Spanned::new(ExprKind::Str(self.current_file.clone()), sp),
                    Spanned::new(ExprKind::Int(sp.start.line as i64), sp),
                ],
            ),
            sp,
        )
    }

    /// Comparison expression for `assert_eq(a, b)`: mirrors `==` semantics —
    /// untyped literals adapt with range checks, mixed int widths need a
    /// cast, floats must match width, strings compare by content.
    fn build_assert_eq(&mut self, l: &mut Expr, r: &mut Expr, sp: Span) -> CResult<Expr> {
        let lt = self.expr_ty(l)?;
        let rt = self.expr_ty(r)?;
        let eq = if is_int(&lt) && is_int(&rt) {
            if lt != rt {
                self.int_op_width(&BinOp::Eq, l, &lt, r, &rt, sp)?;
                if !is_untyped_lit(l) && !is_untyped_lit(r) {
                    return err_at(
                        sp,
                        format!("comparison between {} and {}", lt.name(), rt.name()),
                    );
                }
            }
            let left = std::mem::replace(l, dummy_expr(sp));
            let right = std::mem::replace(r, dummy_expr(sp));
            Spanned::new(ExprKind::Binary(BinOp::Eq, Box::new(left), Box::new(right)), sp)
        } else if lt == Type::Str && rt == Type::Str {
            let left = std::mem::replace(l, dummy_expr(sp));
            let right = std::mem::replace(r, dummy_expr(sp));
            Spanned::new(
                ExprKind::Call("_wlel_streq".into(), vec![], vec![left, right]),
                sp,
            )
        } else if lt == rt && matches!(lt, Type::Bool | Type::Float(_)) {
            let left = std::mem::replace(l, dummy_expr(sp));
            let right = std::mem::replace(r, dummy_expr(sp));
            Spanned::new(ExprKind::Binary(BinOp::Eq, Box::new(left), Box::new(right)), sp)
        } else if is_float_t(&lt) && is_float_t(&rt) {
            // f32 vs f64 compares via a cast in normal code; keep assert_eq
            // as strict as `==` so widths stay explicit
            return err_at(
                sp,
                format!("comparison between {} and {}", lt.name(), rt.name()),
            );
        } else {
            return err_at(
                sp,
                format!(
                    "assert_eq() supports numbers, bools and strings, got {} and {}",
                    lt.name(),
                    rt.name()
                ),
            );
        };
        Ok(eq)
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
/// function plus the file of every used struct or enum. Import paths are
/// resolved to canonical paths, so matching is exact. Generic templates
/// attribute their file when the *template name* is referenced.
fn called_files(
    program: &Program,
    called_funcs: &HashSet<String>,
    used_structs: &HashSet<String>,
    used_enums: &HashSet<String>,
    generic_funcs: &HashMap<String, FuncDef>,
    generic_structs: &HashMap<String, StructDef>,
    generic_enums: &HashMap<String, EnumDef>,
) -> HashSet<String> {
    let mut files: HashSet<String> = HashSet::new();
    for f in &program.funcs {
        if called_funcs.contains(&f.name) {
            files.insert(f.file.clone());
        }
    }
    for s in &program.structs {
        if used_structs.contains(&s.name) {
            files.insert(s.file.clone());
        }
    }
    for e in &program.enums {
        if used_enums.contains(&e.name) {
            files.insert(e.file.clone());
        }
    }
    for (name, f) in generic_funcs {
        if called_funcs.contains(name) {
            files.insert(f.file.clone());
        }
    }
    for (name, s) in generic_structs {
        if used_structs.contains(name) {
            files.insert(s.file.clone());
        }
    }
    for (name, e) in generic_enums {
        if used_enums.contains(name) {
            files.insert(e.file.clone());
        }
    }
    files
}

/// mangle a type string into a C identifier fragment:
/// "*int" -> "pint", "Vec[int]" -> "Vec$int", "[int; 3]" -> "$int3"
fn mangle_ty(t: &str) -> String {
    let mut out = String::new();
    for c in t.chars() {
        match c {
            '*' => out.push('p'),
            '[' => out.push('$'),
            ']' | ';' | ',' | ' ' => {}
            other => out.push(other),
        }
    }
    out
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
        // a statement-form match whose arms all return (the checker proved
        // exhaustiveness when it set all_arms_terminate)
        Some(StmtKind::Match(m)) => m.all_arms_terminate,
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

/// split "int, HashMap[K, V]" on top-level commas (bracket-aware)
fn split_top_level_args(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '[' => {
                depth += 1;
                cur.push(c);
            }
            ']' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ',' if depth == 0 => {
                parts.push(cur.trim().to_string());
                cur.clear();
            }
            other => cur.push(other),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur.trim().to_string());
    }
    parts
}

/// concrete type of a pattern that contains no type variables and no
/// generic arguments (pure lookup — no side effects)
fn resolve_concrete(pattern: &str) -> Type {
    let stars = pattern.chars().take_while(|c| *c == '*').count();
    let base = &pattern[stars..];
    let t = match Type::from_builtin(base) {
        Some(t) => t,
        None => Type::Struct(base.to_string()),
    };
    let mut out = t;
    for _ in 0..stars {
        out = Type::Ptr(Box::new(out));
    }
    out
}

/// how to write a variant value correctly: bare when it has no payloads,
/// called when it does (a payload-less variant is a plain value, a
/// payload-ful one is a constructor call)
fn usage_hint(variant: &str, payload_count: usize, is_call: bool) -> String {
    if payload_count == 0 {
        if is_call {
            format!(" — a payload-less variant is a plain value: write '{variant}'")
        } else {
            String::new()
        }
    } else if !is_call {
        format!(" — a variant with payloads is a constructor: call it, e.g. {variant}(...)")
    } else {
        String::new()
    }
}
