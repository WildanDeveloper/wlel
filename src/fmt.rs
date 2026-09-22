//! `wlel fmt` — the canonical formatter. Parses the source and re-prints it
//! from the AST, so output is a pure function of the parse tree plus the
//! comment positions; formatting twice always yields the same text.
//!
//! Canonical rules:
//! - 4-space indent, one statement / field / parameter per line, no blank
//!   lines inside blocks unless the source had one (collapsed to a single
//!   blank line), exactly one blank line between top-level declarations
//!   (except adjacent `use` lines).
//! - `//` comments are kept: attached above the next statement or
//!   declaration, or trailing on the line of the construct they followed in
//!   the source. Comments in positions the printer cannot anchor (struct
//!   bodies, parameter lists) attach to the next flush point — still stable.
//! - Parenthesization is reconstructed from precedence (the AST drops
//!   redundant parens; `(a + b) * c` keeps its parens, `a + (b * c)` loses
//!   them). Width-suffixed literals print attached: `10u8`, `2.5f32`.
//! - Floats print in shortest-roundtrip form and always stay float-typed
//!   (`1e3` becomes `1000.0`); radix and `_` separators normalize to plain
//!   decimal.

use crate::ast::*;
use crate::lexer::{Comment, Lexer};
use crate::parser::Parser;

pub fn format_source(src: &str) -> Result<String, String> {
    let (tokens, comments) =
        Lexer::new(src).tokenize_with_comments().map_err(|e| e.to_string())?;
    let (program, errors) = Parser::new(&tokens).program();
    if !errors.is_empty() {
        return Err(errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"));
    }
    Ok(Printer::new(comments).render(&program))
}

// ---------------------------------------------------------------------------
// printer
// ---------------------------------------------------------------------------

/// one top-level declaration, tagged with its source line so the original
/// interleaving of uses/structs/enums/impls/funcs/tests is preserved
enum Decl<'a> {
    Use(&'a UseDecl),
    Struct(&'a StructDef),
    Enum(&'a EnumDef),
    Impl(&'a ImplDef),
    Func(&'a FuncDef),
    Test(&'a TestDef),
}

impl Decl<'_> {
    fn start_line(&self) -> usize {
        match self {
            Decl::Use(u) => u.span.start.line,
            Decl::Struct(s) => s.span.start.line,
            Decl::Enum(e) => e.span.start.line,
            Decl::Impl(i) => i.span.start.line,
            Decl::Func(f) => f.span.start.line,
            Decl::Test(t) => t.span.start.line,
        }
    }

    fn end_line(&self) -> usize {
        match self {
            Decl::Use(u) => u.span.end.line,
            Decl::Struct(s) => s.span.end.line,
            Decl::Enum(e) => e.span.end.line,
            Decl::Impl(i) => i.span.end.line,
            Decl::Func(f) => f.span.end.line,
            Decl::Test(t) => t.span.end.line,
        }
    }
}

struct Printer {
    out: String,
    indent: usize,
    comments: Vec<Comment>,
    /// index of the next unemitted comment
    ci: usize,
    /// source line of the last emitted construct (drives blank-line policy)
    last_src: usize,
}

impl Printer {
    fn new(comments: Vec<Comment>) -> Printer {
        Printer { out: String::new(), indent: 0, comments, ci: 0, last_src: 0 }
    }

    fn render(mut self, program: &Program) -> String {
        let mut decls: Vec<Decl> = Vec::new();
        for u in &program.uses {
            decls.push(Decl::Use(u));
        }
        for s in &program.structs {
            decls.push(Decl::Struct(s));
        }
        for e in &program.enums {
            decls.push(Decl::Enum(e));
        }
        for i in &program.impls {
            decls.push(Decl::Impl(i));
        }
        for f in &program.funcs {
            decls.push(Decl::Func(f));
        }
        for t in &program.tests {
            decls.push(Decl::Test(t));
        }
        decls.sort_by_key(|d| d.start_line());

        let mut prev_use = false;
        for d in &decls {
            let is_use = matches!(d, Decl::Use(_));
            if self.last_src > 0 && (!is_use || !prev_use) {
                // exactly one blank line between declarations, but adjacent
                // `use` lines stay grouped
                self.blank();
            }
            self.flush_before(d.start_line());
            match d {
                Decl::Use(u) => self.use_decl(u),
                Decl::Struct(s) => self.struct_def(s),
                Decl::Enum(e) => self.enum_def(e),
                Decl::Impl(i) => self.impl_def(i),
                Decl::Func(f) => self.func_def(f),
                Decl::Test(t) => self.test_def(t),
            }
            self.finish_line(d.end_line());
            prev_use = is_use;
        }
        // comments after the last declaration (or in a comment-only file)
        self.flush_before(usize::MAX);
        for c in self.pending_text() {
            self.put(&c);
        }
        self.out
    }

    fn blank(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn put(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// like `put` but without the trailing newline (multi-line constructs
    /// that start mid-line: `let x := match s {`)
    fn put_no_nl(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
    }

    /// append trailing comment text to the just-closed line, then make sure
    /// the line is terminated. Every declaration and statement funnels
    /// through here so a trailing `// note` lands on the construct's final
    /// line.
    fn finish_line(&mut self, end_line: usize) {
        if self.ci < self.comments.len() && self.comments[self.ci].line <= end_line {
            let mut parts = Vec::new();
            while self.ci < self.comments.len() && self.comments[self.ci].line <= end_line {
                let c = &self.comments[self.ci];
                parts.push(format!("//{}", c.text.trim_end()));
                self.last_src = c.line;
                self.ci += 1;
            }
            if self.out.ends_with('\n') {
                self.out.pop();
            }
            self.out.push_str(&format!(" {}", parts.join(" ")));
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    /// emit pending comments that appear before source line `line`, each on
    /// its own line; source gaps > 1 line become a single blank line
    fn flush_before(&mut self, line: usize) {
        while self.ci < self.comments.len() && self.comments[self.ci].line < line {
            let c = self.comments[self.ci].clone();
            self.ci += 1;
            if c.line.saturating_sub(self.last_src) > 1 {
                self.blank();
            }
            self.put(&format!("//{}", c.text.trim_end()));
            self.last_src = c.line;
        }
    }

    /// remaining comments (EOF flush)
    fn pending_text(&self) -> Vec<String> {
        self.comments[self.ci..]
            .iter()
            .map(|c| format!("//{}", c.text.trim_end()))
            .collect()
    }

    fn use_decl(&mut self, u: &UseDecl) {
        match &u.path {
            Some(p) => self.put(&format!("use \"{}\";", escape_string(p))),
            None => self.put("use std;"),
        }
        self.last_src = u.span.end.line;
    }

    fn struct_def(&mut self, s: &StructDef) {
        let mut head = format!("struct {}", s.name);
        if !s.type_params.is_empty() {
            head.push_str(&format!("[{}]", s.type_params.join(", ")));
        }
        if s.fields.is_empty() {
            self.put(&format!("{head} {{}}"));
        } else {
            self.put(&format!("{head} {{"));
            self.indent += 1;
            for (i, (fname, fty)) in s.fields.iter().enumerate() {
                let comma = if i + 1 < s.fields.len() { "," } else { "" };
                self.put(&format!("{fname}: {fty}{comma}"));
            }
            self.indent -= 1;
            self.put("}");
        }
        self.last_src = s.span.end.line;
    }

    fn enum_def(&mut self, e: &EnumDef) {
        let mut head = format!("enum {}", e.name);
        if !e.type_params.is_empty() {
            head.push_str(&format!("[{}]", e.type_params.join(", ")));
        }
        if e.variants.is_empty() {
            self.put(&format!("{head} {{}}"));
        } else {
            self.put(&format!("{head} {{"));
            self.indent += 1;
            for (i, v) in e.variants.iter().enumerate() {
                let comma = if i + 1 < e.variants.len() { "," } else { "" };
                if v.payloads.is_empty() {
                    self.put(&format!("{}{comma}", v.name));
                } else {
                    self.put(&format!("{}({}){comma}", v.name, v.payloads.join(", ")));
                }
            }
            self.indent -= 1;
            self.put("}");
        }
        self.last_src = e.span.end.line;
    }

    /// `impl Name[T, ...] { fn method ... }` — methods print like top-level
    /// functions, one blank line between them
    fn impl_def(&mut self, i: &ImplDef) {
        let mut head = format!("impl {}", i.type_name);
        if !i.type_params.is_empty() {
            head.push_str(&format!("[{}]", i.type_params.join(", ")));
        }
        self.put(&format!("{head} {{"));
        self.last_src = i.span.start.line;
        self.indent += 1;
        for m in &i.methods {
            self.blank();
            self.func_def(m);
        }
        self.indent -= 1;
        self.put("}");
        self.last_src = i.span.end.line;
    }

    fn func_def(&mut self, f: &FuncDef) {
        if f.is_extern {
            self.put(&format!("extern {};", self.func_head(f)));
            self.last_src = f.span.end.line;
            return;
        }
        self.put(&self.func_head(f));
        // comments inside the body measure their gap from the signature
        self.last_src = f.span.start.line;
        self.block_body(&f.body, f.span.end.line);
        self.last_src = f.span.end.line;
    }

    /// `fn name[T](params) -> ret` without braces (func_def appends ` {`),
    /// or the full `extern fn name(params) -> ret` when is_extern
    fn func_head(&self, f: &FuncDef) -> String {
        let mut head = format!("fn {}", f.name);
        if !f.type_params.is_empty() {
            head.push_str(&format!("[{}]", f.type_params.join(", ")));
        }
        let params = f
            .params
            .iter()
            .map(|p| match &p.ty {
                Some(t) => format!("{}: {}", p.name, t),
                None => p.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        head.push_str(&format!("({params})"));
        if let Some(r) = &f.ret_type {
            head.push_str(&format!(" -> {r}"));
        }
        if f.is_extern {
            head
        } else {
            format!("{head} {{")
        }
    }

    fn test_def(&mut self, t: &TestDef) {
        self.put(&format!("test \"{}\" {{", escape_string(&t.name)));
        self.last_src = t.span.start.line;
        self.block_body(&t.body, t.span.end.line);
        self.last_src = t.span.end.line;
    }

    /// statements of an already-opened block; `end_line` is the source line
    /// of the closing brace so end-of-block comments stay inside
    fn block_body(&mut self, b: &Block, end_line: usize) {
        self.indent += 1;
        for s in &b.0 {
            self.stmt(s);
        }
        self.flush_before(end_line);
        self.indent -= 1;
        self.put("}");
    }

    fn stmt(&mut self, s: &Stmt) {
        self.flush_before(s.span.start.line);
        // comments inside this statement's blocks measure their gap from the
        // statement itself, not from the previous sibling
        self.last_src = s.span.start.line;
        match &s.node {
            StmtKind::Let(name, ty, e) if matches!(&e.node, ExprKind::Match(_)) => {
                let head = match ty {
                    Some(t) => format!("let {name}: {t} = "),
                    None => format!("{name} := "),
                };
                if let ExprKind::Match(m) = &e.node {
                    let scrut = self.expr(&m.scrutinee, 0);
                    self.put_no_nl(&format!("{head}match {scrut} {{"));
                    self.out.push('\n');
                    self.match_close(m, "};", s.span.end.line);
                }
            }
            StmtKind::Let(name, Some(ty), e) => {
                let v = self.expr(e, 0);
                self.put(&format!("let {name}: {ty} = {v};"));
            }
            StmtKind::Let(name, None, e) => {
                let v = self.expr(e, 0);
                self.put(&format!("{name} := {v};"));
            }
            StmtKind::Assign(a) => {
                let op = match a.op {
                    CompoundOp::Set => "=",
                    CompoundOp::Add => "+=",
                    CompoundOp::Sub => "-=",
                    CompoundOp::Mul => "*=",
                    CompoundOp::Div => "/=",
                    CompoundOp::Mod => "%=",
                };
                let t = self.expr(&a.target, 0);
                let v = self.expr(&a.value, 0);
                self.put(&format!("{t} {op} {v};"));
            }
            StmtKind::If(i) => self.if_stmt(i, s.span.end.line),
            StmtKind::While(cond, body) => {
                let c = self.expr(cond, 0);
                self.put(&format!("while {c} {{"));
                self.block_body(body, s.span.end.line);
            }
            StmtKind::For(var, from, to, body) => {
                let a = self.expr(from, 0);
                let b = self.expr(to, 0);
                self.put(&format!("for {var} in {a}..{b} {{"));
                self.block_body(body, s.span.end.line);
            }
            StmtKind::ForIn(f) => {
                let it = self.expr(&f.iter, 0);
                self.put(&format!("for {} in {it} {{", f.var));
                self.block_body(&f.body, s.span.end.line);
            }
            StmtKind::Break => self.put("break;"),
            StmtKind::Continue => self.put("continue;"),
            StmtKind::Return(e) => match e {
                Some(e) => {
                    if let ExprKind::Match(m) = &e.node {
                        let scrut = self.expr(&m.scrutinee, 0);
                        self.put_no_nl(&format!("return match {scrut} {{"));
                        self.out.push('\n');
                        self.match_close(m, "};", s.span.end.line);
                    } else {
                        let v = self.expr(e, 0);
                        self.put(&format!("return {v};"));
                    }
                }
                None => self.put("return;"),
            },
            StmtKind::Match(m) => {
                let scrut = self.expr(&m.scrutinee, 0);
                self.put_no_nl(&format!("match {scrut} {{"));
                self.out.push('\n');
                self.match_close(m, "}", s.span.end.line);
            }
            StmtKind::ExprStmt(e) => {
                let v = self.expr(e, 0);
                self.put(&format!("{v};"));
            }
            StmtKind::Defer(DeferBody::Expr(e)) => {
                let v = self.expr(e, 0);
                self.put(&format!("defer {v};"));
            }
            StmtKind::Defer(DeferBody::Block(b)) => {
                self.put("defer {");
                self.block_body(b, s.span.end.line);
            }
            StmtKind::Arena(cap, body) => {
                match cap {
                    Some(c) => {
                        let v = self.expr(c, 0);
                        self.put(&format!("arena({v}) {{"));
                    }
                    None => self.put("arena {"),
                }
                self.block_body(body, s.span.end.line);
            }
            StmtKind::Block(b) => {
                self.put("{");
                self.block_body(b, s.span.end.line);
            }
        }
        self.finish_line(s.span.end.line);
        self.last_src = s.span.end.line;
    }

    fn if_stmt(&mut self, i: &IfStmt, end_line: usize) {
        let cond = self.expr(&i.cond, 0);
        self.put(&format!("if {cond} {{"));
        self.if_tail(i, end_line);
    }

    /// pattern text: `_` or `Variant` or `Variant(a, b)`
    fn pattern_str(&self, p: &Pattern) -> String {
        match p {
            Pattern::Wildcard => "_".into(),
            Pattern::Variant { name, binds, .. } => {
                if binds.is_empty() {
                    name.clone()
                } else {
                    let b = binds
                        .iter()
                        .map(|x| x.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name}({b})")
                }
            }
        }
    }

    /// arms of an already-opened `match scrut {` line, then the closing
    /// line (`};` in value form, `}` in statement form). Expression arms
    /// carry a trailing comma; block arms none — both reparse identically.
    fn match_close(&mut self, m: &MatchStmt, close: &str, end_line: usize) {
        self.indent += 1;
        for arm in &m.arms {
            self.flush_before(arm.span.start.line);
            self.last_src = arm.span.start.line;
            let pat = self.pattern_str(&arm.pattern);
            self.put_no_nl(&format!("{pat} => "));
            match &arm.body {
                MatchBody::Expr(x) => {
                    let v = self.expr(x, 0);
                    self.out.push_str(&v);
                    self.out.push_str(",\n");
                    self.finish_line(arm.span.end.line);
                }
                MatchBody::Block(b) => {
                    self.out.push_str("{\n");
                    self.block_body(b, arm.span.end.line);
                }
            }
        }
        self.indent -= 1;
        self.flush_before(end_line);
        self.put(close);
    }

    /// branches + optional `else` chain after an already-printed `if cond {`
    fn if_tail(&mut self, i: &IfStmt, end_line: usize) {
        // a then-branch followed by `else` has no reliable closing-brace
        // line: bound comments by the line before the else arm, or leave
        // them to the next flush point
        let then_end = match &i.else_branch {
            Some(ElseBranch::If(nested)) => Some(nested.cond.span.start.line - 1),
            Some(ElseBranch::Block(b)) => b.0.first().map(|s| s.span.start.line - 1),
            None => Some(end_line),
        };
        match then_end {
            Some(l) => self.block_body(&i.then_body, l),
            None => {
                self.indent += 1;
                for st in &i.then_body.0 {
                    self.stmt(st);
                }
                self.indent -= 1;
                self.put("}");
            }
        }
        match &i.else_branch {
            Some(ElseBranch::If(nested)) => {
                self.out.pop();
                self.out.push_str(" else ");
                let cond = self.expr(&nested.cond, 0);
                self.out.push_str(&format!("if {cond} {{\n"));
                self.if_tail(nested, end_line);
            }
            Some(ElseBranch::Block(b)) => {
                self.out.pop();
                self.out.push_str(" else {");
                self.block_body(b, end_line);
            }
            None => {}
        }
    }

    // -- expressions --------------------------------------------------------

    /// precedence of a node as an operand (higher binds tighter); atoms and
    /// postfix chains outrank everything, `as` sits just below unary
    fn prec(e: &Expr) -> u8 {
        match &e.node {
            ExprKind::Binary(op, _, _) => match op {
                BinOp::Or => 2,
                BinOp::And => 4,
                BinOp::BitOr => 6,
                BinOp::BitXor => 8,
                BinOp::BitAnd => 10,
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => 12,
                BinOp::Shl | BinOp::Shr => 14,
                BinOp::Add | BinOp::Sub => 16,
                BinOp::Mul | BinOp::Div | BinOp::Mod => 18,
            },
            ExprKind::Cast(_, _) => 20,
            ExprKind::Unary(_, _) | ExprKind::AddrOf(_) | ExprKind::Deref(_) => 21,
            _ => 100,
        }
    }

    fn op_str(op: BinOp) -> &'static str {
        match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }

    /// print `e` as an operand that must bind at least `min_prec` — a looser
    /// child is wrapped in parentheses (the AST keeps no paren nodes)
    fn expr(&mut self, e: &Expr, min_prec: u8) -> String {
        let p = Self::prec(e);
        let body = self.expr_inner(e);
        if p < min_prec {
            format!("({body})")
        } else {
            body
        }
    }

    /// unary-strength operand: looser binaries/casts parenthesize, other
    /// unaries parenthesize too (conservative but stable: `-(-x)`)
    fn un_operand(&mut self, e: &Expr) -> String {
        match &e.node {
            ExprKind::Binary(_, _, _) | ExprKind::Cast(_, _) | ExprKind::Unary(_, _) => {
                format!("({})", self.expr(e, 0))
            }
            _ => self.expr(e, 0),
        }
    }

    fn expr_inner(&mut self, e: &Expr) -> String {
        match &e.node {
            ExprKind::Int(v) => v.to_string(),
            ExprKind::UInt(v) => v.to_string(),
            ExprKind::Float(v) => float_lit(*v),
            ExprKind::Str(s) => format!("\"{}\"", escape_string(s)),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Ident(n) => n.clone(),
            ExprKind::Unary(op, inner) => {
                let sym = match op {
                    UnOp::Neg => "-",
                    UnOp::Not => "!",
                    UnOp::BitNot => "~",
                };
                format!("{sym}{}", self.un_operand(inner))
            }
            ExprKind::AddrOf(inner) => format!("&{}", self.un_operand(inner)),
            ExprKind::Deref(inner) => format!("*{}", self.un_operand(inner)),
            ExprKind::Binary(op, l, r) => {
                let p = Self::prec(e);
                format!(
                    "{} {} {}",
                    self.expr(l, p),
                    Self::op_str(*op),
                    self.expr(r, p + 1)
                )
            }
            ExprKind::Call(name, ty_args, args) => {
                let mut s = name.clone();
                if !ty_args.is_empty() {
                    s.push_str(&format!("[{}]", ty_args.join(", ")));
                }
                let a = args
                    .iter()
                    .map(|x| self.expr(x, 0))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{s}({a})")
            }
            ExprKind::Field(base, name) => format!("{}.{}", self.expr(base, 100), name),
            ExprKind::MethodCall(base, name, args) => {
                let a = args
                    .iter()
                    .map(|x| self.expr(x, 0))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}.{}({})", self.expr(base, 100), name, a)
            }
            ExprKind::StructLit(name, fields) => {
                if fields.is_empty() {
                    format!("{name} {{ }}")
                } else {
                    let f = fields
                        .iter()
                        .map(|(k, v)| format!("{k}: {}", self.expr(v, 0)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name} {{ {f} }}")
                }
            }
            ExprKind::Cast(ty, inner) => {
                // width-suffixed literals keep their attached form: `10u8`
                if let Some(lit) = literal_suffix_form(ty, inner) {
                    return lit;
                }
                format!("{} as {ty}", self.expr(inner, 20))
            }
            ExprKind::Index(base, idx) => {
                format!("{}[{}]", self.expr(base, 100), self.expr(idx, 0))
            }
            ExprKind::ArrayLit(elems) => {
                let a = elems
                    .iter()
                    .map(|x| self.expr(x, 0))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{a}]")
            }
            ExprKind::Sizeof(ty) => format!("wlel_sizeof({ty})"),
            ExprKind::New(ty, count) => match count {
                Some(n) => format!("new({ty}, {})", self.expr(n, 0)),
                None => format!("new({ty})"),
            },
            ExprKind::CurrentArena => "wlel_arena()".into(),
            // enum constructors print as their original call form (fmt runs
            // on parse output, so this node only appears post-check)
            ExprKind::EnumLit(vname, _, _, payloads) => {
                let a = payloads
                    .iter()
                    .map(|x| self.expr(x, 0))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{vname}({a})")
            }
            // a match only ever occupies statement level (the parser only
            // accepts it as a statement or a let/return initializer, both
            // printed by their own arms above)
            ExprKind::Match(_) => unreachable!("match outside statement position"),
            ExprKind::Try(t) => format!("{}?", self.expr(&t.inner, 100)),
        }
    }
}

/// `2.5f32` / `10u8` — a cast applied directly to a non-negative numeric
/// literal prints in attached-suffix form (it reparses to exactly the same
/// AST); everything else prints as `expr as Type`
fn literal_suffix_form(ty: &str, inner: &Expr) -> Option<String> {
    const WIDTHS: [&str; 11] = [
        "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "usize", "f32", "f64",
    ];
    if !WIDTHS.contains(&ty) && ty != "byte" && ty != "char" {
        return None;
    }
    match &inner.node {
        ExprKind::Int(v) if *v >= 0 => Some(format!("{v}{ty}")),
        ExprKind::UInt(v) => Some(format!("{v}{ty}")),
        ExprKind::Float(v) => Some(format!("{}{ty}", float_lit(*v))),
        _ => None,
    }
}

/// shortest round-trip float text that still lexes as a float literal
fn float_lit(v: f64) -> String {
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out
}
