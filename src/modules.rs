//! Module namespaces.
//!
//! Wlel used to flatten every imported file into one global scope, so two
//! files that both defined `helper` were a duplicate-symbol error. That is
//! what made a multi-file project impossible to grow past a certain size, and
//! it was the blocker for self-hosting.
//!
//! The model now:
//!
//! - Every file belongs to a module. `module geom;` names it; without the
//!   declaration the file stem is used (`lib/geom.wlel` -> `geom`).
//! - The file the user asked to run or build is the **root module**: its items
//!   stay unqualified, so `fn main` is still the C entry point and a
//!   single-file program behaves exactly as before.
//! - `use "lib/geom.wlel";` binds the module `geom`; its items are reached as
//!   `geom::add`, `geom::Vec2`.
//! - `use "lib/geom.wlel" { add, Vec2 };` also brings those names into the
//!   importing module's own scope. `{ * }` brings all of them, which is what
//!   `use std;` means.
//! - A bare name resolves to the current module first, then to the names
//!   imported into it. Two candidates at the same step is an ambiguity error
//!   that names both and tells the reader to qualify — never a silent pick.
//!
//! Declarations are qualified in place (`geom::add`) so diagnostics stay
//! readable, and [`mangle`] rewrites the `::` separators to `__` once, at the
//! end of the pipeline, because `::` is not valid in a C identifier.

use crate::ast::*;

/// The separator a module-qualified name is emitted with in C. `__` cannot
/// appear in a source identifier (the parser rejects it), so a mangled name
/// can never collide with a user name.
pub const SEP: &str = "__";

/// `geom::add` -> `geom__add`; leaves unqualified names alone.
pub fn mangle(name: &str) -> String {
    name.replace("::", SEP)
}

/// Rewrite every `::` in a type string (`geom::Vec2` -> `geom__Vec2`,
/// `*geom::Vec2` too). Array and generic brackets are preserved.
pub fn mangle_type(ty: &str) -> String {
    if !ty.contains("::") {
        return ty.to_string();
    }
    ty.replace("::", SEP)
}

/// Tag every function with the module it belongs to, so the checker can
/// resolve a bare name against that module's imports.
pub fn tag_module(program: &mut Program, module: &str) {
    for f in program.funcs.iter_mut() {
        f.module = module.to_string();
    }
    for i in program.impls.iter_mut() {
        for m in i.methods.iter_mut() {
            m.module = module.to_string();
        }
    }
    for t in program.tests.iter_mut() {
        t.module = module.to_string();
    }
}

/// Qualify one file's top-level items with its module name.
///
/// The root module is left untouched: its names are the program's own, and
/// `main` must stay `main`.
pub fn qualify(program: &mut Program, module: &str, is_root: bool) {
    program.module = module.to_string();
    if is_root || module.is_empty() {
        return;
    }
    let q = |name: &str| format!("{module}::{name}");
    for f in program.funcs.iter_mut() {
        f.name = q(&f.name);
    }
    for s in program.structs.iter_mut() {
        s.name = q(&s.name);
    }
    for e in program.enums.iter_mut() {
        // the enum is module-qualified; its variants stay short and are
        // namespaced by the enum (`geom::Shape::Circle`, not `geom::Circle`)
        e.name = q(&e.name);
    }
    for i in program.impls.iter_mut() {
        i.type_name = q(&i.type_name);
        // method names stay short: they are looked up as
        // `(impl type, method)` and only the desugared function is qualified
    }
}

/// The exports of one file, in source terms (unqualified).
///
/// This is computed *before* [`qualify`] runs, so the table always records the
/// names a user writes (`add`), and resolution maps them onto whatever the
/// declaration was qualified to.
pub fn exports_of(program: &Program) -> Vec<Export> {
    let mut out: Vec<Export> = Vec::new();
    let unq = |n: &str| n.rsplit("::").next().unwrap_or(n).to_string();
    for f in &program.funcs {
        if f.is_extern {
            // an extern declaration binds a C symbol; it is not part of the
            // module's Wlel-level API
            continue;
        }
        out.push(Export {
            name: unq(&f.name),
            qualified: f.name.clone(),
            kind: ExportKind::Function,
            owner: String::new(),
        });
    }
    for s in &program.structs {
        out.push(Export {
            name: unq(&s.name),
            qualified: s.name.clone(),
            kind: ExportKind::Struct,
            owner: String::new(),
        });
    }
    for e in &program.enums {
        out.push(Export {
            name: unq(&e.name),
            qualified: e.name.clone(),
            kind: ExportKind::Enum,
            owner: e.name.clone(),
        });
        for v in &e.variants {
            out.push(Export {
                name: unq(&v.name),
                qualified: format!("{}::{}", e.name, v.name),
                kind: ExportKind::Variant,
                owner: e.name.clone(),
            });
        }
    }
    // dedup by the *qualified* name: two enums may both export a variant
    // called `Same`, and collapsing them would silently pick one
    out.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.kind.cmp(&b.kind))
            .then(a.qualified.cmp(&b.qualified))
    });
    out.dedup_by(|a, b| a.name == b.name && a.kind == b.kind && a.qualified == b.qualified);
    out
}

/// The module name a `use "path"` binds: the declared name if the file has a
/// `module` line, otherwise the file stem.
pub fn bind_name(declared: &str, path: &str) -> String {
    if !declared.is_empty() {
        return declared.to_string();
    }
    let stem = path.rsplit('/').next().unwrap_or(path);
    stem.strip_suffix(".wlel").unwrap_or(stem).to_string()
}

/// The name an import is reached by in source: `as` alias, else the module.
pub fn local_name(use_decl: &UseDecl) -> String {
    use_decl.alias.clone().unwrap_or_else(|| use_decl.module.clone())
}

// ---------------------------------------------------------------------------
// name resolution
// ---------------------------------------------------------------------------

/// What a written name resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// the declaration's qualified name (`geom::add`)
    Item(String),
    /// a module bound by a `use` (`geom`); only valid before `::`
    Module(String),
    /// nothing claims this name
    Unknown,
}

/// Resolve `name` as written inside `current_module`.
///
/// Order: the module's own items, then its imports. An explicit `{ a }` import
/// beats the implicit "everything from a plain `use`" binding; two candidates
/// at the same level is an error the caller reports, never a silent pick.
pub fn resolve<S: std::hash::BuildHasher>(
    name: &str,
    current_module: &str,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) -> Result<Resolved, Ambiguity> {
    // `std::println_int` and `sys::...` are handled by the checker's builtin
    // arms; anything else with a `::` is a module path.
    if let Some((head, rest)) = name.split_once("::") {
        if let Some(bindings) = table.get(current_module) {
            if let Some(b) = bindings.iter().find(|b| b.is_module && b.local == head) {
                return Ok(Resolved::Item(format!("{}::{rest}", b.target)));
            }
        }
        // already qualified in source (`geom::add` inside geom itself)
        if table.contains_key(head) {
            return Ok(Resolved::Item(name.to_string()));
        }
        return Ok(Resolved::Unknown);
    }

    // 1. the module's own items. Two enums may share a variant name, so a
    //    name can match more than once — that is an ambiguity, not a pick.
    if let Some(list) = exports.get(current_module) {
        let mut own: Vec<&Export> = list.iter().filter(|e| e.name == name).collect();
        match own.len() {
            0 => {}
            1 => return Ok(Resolved::Item(own[0].qualified.clone())),
            _ => {
                own.sort_by(|a, b| a.qualified.cmp(&b.qualified));
                let from: Vec<String> = own.iter().map(|e| e.qualified.clone()).collect();
                return Err(Ambiguity { name: name.to_string(), from });
            }
        }
    }
    let Some(bindings) = table.get(current_module) else {
        return Ok(Resolved::Unknown);
    };
    // 2. a module of that name (only meaningful before `::`, but binding it
    //    lets the caller produce "did you mean geom::add?")
    if let Some(b) = bindings.iter().find(|b| b.is_module && b.local == name) {
        return Ok(Resolved::Module(b.target.clone()));
    }
    // 3. imported items
    let mut candidates: Vec<&ImportBinding> =
        bindings.iter().filter(|b| !b.is_module && b.local == name).collect();
    if candidates.is_empty() {
        return Ok(Resolved::Unknown);
    }
    let explicit: Vec<&ImportBinding> = candidates.iter().copied().filter(|b| b.explicit).collect();
    if !explicit.is_empty() {
        candidates = explicit;
    }
    candidates.dedup_by(|a, b| a.target == b.target);
    match candidates.len() {
        0 => Ok(Resolved::Unknown),
        1 => Ok(Resolved::Item(candidates[0].target.clone())),
        _ => {
            let mut from: Vec<String> = candidates.iter().map(|b| b.target.clone()).collect();
            from.sort();
            from.dedup();
            Err(Ambiguity { name: name.to_string(), from })
        }
    }
}

/// Two imports claim the same bare name.
#[derive(Debug, Clone)]
pub struct Ambiguity {
    pub name: String,
    pub from: Vec<String>,
}

impl std::fmt::Display for Ambiguity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "'{}' is ambiguous — it could be {}; qualify it, or import the one you \
             want with `use \"…\" {{ {} }};`",
            self.name,
            self.from
                .iter()
                .map(|t| format!("`{t}`"))
                .collect::<Vec<_>>()
                .join(" or "),
            self.name
        )
    }
}

/// Every module a name could come from, for "did you mean" hints.
pub fn suggest<S: std::hash::BuildHasher>(
    name: &str,
    current_module: &str,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) -> Option<String> {
    let bindings = table.get(current_module)?;
    let mut hits: Vec<String> = bindings
        .iter()
        .filter(|b| !b.is_module && b.local == name)
        .map(|b| b.target.clone())
        .collect();
    hits.sort();
    hits.dedup();
    hits.first().cloned()
}

/// Resolve a name that must be a type (struct, enum or a builtin).
pub fn resolve_type<S: std::hash::BuildHasher>(
    name: &str,
    current_module: &str,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) -> Option<String> {
    let ok = |e: &Export| matches!(e.kind, ExportKind::Struct | ExportKind::Enum);
    if let Some((head, rest)) = name.split_once("::") {
        let module = table
            .get(current_module)
            .and_then(|bs| bs.iter().find(|b| b.is_module && b.local == head))
            .map(|b| b.target.clone())
            .unwrap_or_else(|| head.to_string());
        return exports
            .get(&module)
            .and_then(|list| list.iter().find(|e| e.name == rest && ok(e)))
            .map(|e| e.qualified.clone());
    }
    if let Some(list) = exports.get(current_module) {
        if let Some(e) = list.iter().find(|e| e.name == name && ok(e)) {
            return Some(e.qualified.clone());
        }
    }
    let mut hits: Vec<String> = table
        .get(current_module)
        .map(|bs| {
            bs.iter()
                .filter(|b| !b.is_module && b.local == name)
                .filter(|b| {
                    // the binding target must be a type
                    exports
                        .values()
                        .flatten()
                        .any(|e| e.qualified == b.target && ok(e))
                })
                .map(|b| b.target.clone())
                .collect()
        })
        .unwrap_or_default();
    hits.sort();
    hits.dedup();
    if hits.len() == 1 {
        hits.pop()
    } else {
        None
    }
}

/// Rewrite every type name in `ty` through the module table.
///
/// `*Vec2`, `[Vec2; 3]`, `Box[Vec2]` and `geom::Vec2` all work; builtins and
/// type parameters are left alone because they resolve to nothing.
pub fn qualify_type<S: std::hash::BuildHasher>(
    ty: &str,
    current_module: &str,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) -> String {
    if ty.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = ty.chars().collect();
    let mut out = String::with_capacity(ty.len() + 8);
    let mut ident = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphanumeric() || c == '_' {
            ident.push(c);
            i += 1;
            continue;
        }
        if c == ':' && chars.get(i + 1) == Some(&':') && !ident.is_empty() {
            ident.push_str("::");
            i += 2;
            continue;
        }
        if !ident.is_empty() {
            match resolve_type(&ident, current_module, exports, table) {
                Some(q) => out.push_str(&q),
                None => out.push_str(&ident),
            }
            ident.clear();
        }
        out.push(c);
        i += 1;
    }
    if !ident.is_empty() {
        match resolve_type(&ident, current_module, exports, table) {
            Some(q) => out.push_str(&q),
            None => out.push_str(&ident),
        }
    }
    out
}

/// Rewrite every type annotation and literal name in a program through the
/// module table, in place.
///
/// Runs after the loader has assembled the table and before the checker, so
/// the checker only ever sees fully qualified names.
pub fn qualify_program_types<S: std::hash::BuildHasher>(
    program: &mut Program,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) {
    let module = program.module.clone();
    let q = |ty: &str, m: &str| qualify_type(ty, m, exports, table);
    for f in program.funcs.iter_mut() {
        let m = f.module.clone();
        if let Some(t) = f.ret_type.as_mut() {
            *t = q(t, &m);
        }
        for p in f.params.iter_mut() {
            if let Some(t) = p.ty.as_mut() {
                *t = q(t, &m);
            }
        }
        let m2 = f.module.clone();
        qualify_block_types(&mut f.body, &m2, exports, table);
    }
    for t in program.tests.iter_mut() {
        qualify_block_types(&mut t.body, &module, exports, table);
    }
    for i in program.impls.iter_mut() {
        let m = i.file.clone();
        let m = program
            .modules
            .iter()
            .find(|mi| mi.file == m)
            .map(|mi| mi.name.clone())
            .unwrap_or_default();
        for mt in i.methods.iter_mut() {
            if let Some(t) = mt.ret_type.as_mut() {
                *t = q(t, &m);
            }
            for p in mt.params.iter_mut() {
                if let Some(t) = p.ty.as_mut() {
                    *t = q(t, &m);
                }
            }
            let m3 = mt.module.clone();
            qualify_block_types(&mut mt.body, &m3, exports, table);
        }
    }
    for s in program.structs.iter_mut() {
        let m = s.module.clone();
        for (_, t) in s.fields.iter_mut() {
            *t = q(t, &m);
        }
    }
    for e in program.enums.iter_mut() {
        let m = e.module.clone();
        for v in e.variants.iter_mut() {
            for t in v.payloads.iter_mut() {
                *t = q(t, &m);
            }
        }
    }
}

fn qualify_block_types<S: std::hash::BuildHasher>(
    b: &mut Block,
    module: &str,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) {
    for s in b.0.iter_mut() {
        qualify_stmt_types(s, module, exports, table);
    }
}

fn qualify_if_types<S: std::hash::BuildHasher>(
    i: &mut IfStmt,
    module: &str,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) {
    qualify_block_types(&mut i.then_body, module, exports, table);
    match i.else_branch.as_mut() {
        Some(ElseBranch::Block(b)) => qualify_block_types(b, module, exports, table),
        Some(ElseBranch::If(inner)) => qualify_if_types(inner, module, exports, table),
        None => {}
    }
}

fn qualify_stmt_types<S: std::hash::BuildHasher>(
    s: &mut Stmt,
    module: &str,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) {
    let q = |ty: &str| qualify_type(ty, module, exports, table);
    match &mut s.node {
        StmtKind::Let(_, Some(t), _) => *t = q(t),
        StmtKind::If(i) => {
            qualify_block_types(&mut i.then_body, module, exports, table);
            match i.else_branch.as_mut() {
                Some(ElseBranch::Block(b)) => qualify_block_types(b, module, exports, table),
                Some(ElseBranch::If(inner)) => {
                    qualify_if_types(inner, module, exports, table)
                }
                None => {}
            }
        }
        StmtKind::While(_, b) => qualify_block_types(b, module, exports, table),
        StmtKind::ForIn(f) => {
            if let Some(e) = f.elem.as_mut() {
                *e = q(e);
            }
            qualify_block_types(&mut f.body, module, exports, table);
        }
        StmtKind::For(_, _, _, b) => qualify_block_types(b, module, exports, table),
        StmtKind::Defer(DeferBody::Block(b)) => {
            qualify_block_types(b, module, exports, table)
        }
        StmtKind::Arena(_, b) => qualify_block_types(b, module, exports, table),
        StmtKind::Match(m) => {
            for arm in m.arms.iter_mut() {
                if let MatchBody::Block(b) = &mut arm.body {
                    qualify_block_types(b, module, exports, table);
                }
            }
        }
        StmtKind::Block(b) => qualify_block_types(b, module, exports, table),
        _ => {}
    }
}

/// Rewrite the target of every `impl` block through the module table.
///
/// `impl Vec2 { ... }` inside `geom` and `impl geom::Vec2 { ... }` in another
/// file both have to name the same type, or the method table is keyed by a
/// name no receiver ever matches.
pub fn qualify_impl_targets<S: std::hash::BuildHasher>(
    program: &mut Program,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
    table: &std::collections::HashMap<String, Vec<ImportBinding>, S>,
) {
    // impl blocks are qualified with their own file's module, which the
    // loader already recorded in the method tags
    for i in program.impls.iter_mut() {
        let module = i
            .methods
            .first()
            .map(|m| m.module.clone())
            .unwrap_or_default();
        i.type_name = qualify_type(&i.type_name, &module, exports, table);
        for m in i.methods.iter_mut() {
            // `fn len(self) -> Vec2` inside the module: the return type is a
            // type name too
            if let Some(t) = m.ret_type.as_mut() {
                *t = qualify_type(t, &module, exports, table);
            }
        }
    }
}

/// Reject a program where two files claim one module name, or a file claims
/// the compiler-provided `std`/`sys` roots.
///
/// The qualified name `geom::add` has to mean one thing; two `module geom;`
/// files would silently shadow each other.
pub fn check_unique(program: &Program) -> Result<(), String> {
    let mut seen: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for m in &program.modules {
        if m.name.is_empty() {
            continue;
        }
        if m.name == "std" || m.name == "sys" {
            return Err(format!(
                "{}: the module name '{}' is reserved for the compiler-provided module",
                m.file, m.name
            ));
        }
        if let Some(prev) = seen.insert(m.name.as_str(), m.file.as_str()) {
            if prev != m.file {
                return Err(format!(
                    "two files declare `module {};` — {} and {}; module names must be unique",
                    m.name, prev, m.file
                ));
            }
        }
    }
    Ok(())
}

/// A hint for a name that resolved to nothing: another module exports it.
///
/// `Pt` in a file that never imported `geomlib` used to work because every
/// dependency was flattened into one scope. Now the message says which module
/// has it and what to write.
pub fn hint<S: std::hash::BuildHasher>(
    name: &str,
    current_module: &str,
    exports: &std::collections::HashMap<String, Vec<Export>, S>,
) -> Option<String> {
    let mut hits: Vec<String> = Vec::new();
    for (module, list) in exports {
        if module == current_module || module.is_empty() {
            continue;
        }
        if list.iter().any(|e| e.name == name) {
            hits.push(module.clone());
        }
    }
    hits.sort();
    hits.dedup();
    let first = hits.first()?;
    Some(format!(
        " — '{name}' is in module '{first}'; write `{first}::{name}` or add `use {first};`"
    ))
}
