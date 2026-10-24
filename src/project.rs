//! Project mode: `wlel.toml` manifests, `wlel new` scaffolding, dependency
//! resolution (path + git), and the `wlel.lock` revision pin file.
//!
//! Layout conventions for a project directory:
//!
//! ```text
//! myapp/
//!   wlel.toml      [package] name/version, optional [deps]
//!   src/main.wlel    binary entry (required for `run`/`build`)
//!   src/lib.wlel     library entry (`check`/`test` work on library-only
//!                  projects; every dependency must provide one)
//!   .wlel/deps/    git dependency clones (generated, gitignore it)
//!   wlel.lock      resolved git revisions (generated; commit it)
//! ```
//!
//! The manifest is a strict TOML subset — sections `[package]`/`[deps]`,
//! quoted string values, one-line inline tables for dependencies:
//!
//! ```toml
//! [package]
//! name = "myapp"
//! version = "0.1.0"
//!
//! [deps]
//! pointlib = { path = "../pointlib" }
//! json     = { git = "https://github.com/user/wlel-json" }
//! ```
//!
//! Dependencies merge into the program like file imports (diamond-deduped),
//! so their functions/structs/tests flow through the normal pipeline and
//! only what the program actually uses is emitted. Nested dependencies are
//! resolved recursively; two deps sharing a name with different sources are
//! rejected. Git dependencies may pin a caret version requirement
//! (`version = "1.2"`) — resolved against the remote's git tags and verified
//! against the dependency's own manifest (see [`crate::registry`]); the
//! requirement and resolved revision are both recorded in `wlel.lock`.

use crate::ast::{
    Export, ImportBinding, ImportNames, ModuleInfo, Program, UseDecl,
};
use crate::lexer::Lexer;
use crate::modules;
use crate::parser::Parser;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MANIFEST_FILE: &str = "wlel.toml";
pub const LOCK_FILE: &str = "wlel.lock";
/// generated git-clone cache inside the (root) project directory
const DEPS_DIR: &str = ".wlel/deps";
/// guards against dependency cycles
const MAX_DEP_DEPTH: u32 = 16;

// ---------------------------------------------------------------------------
// manifest (wlel.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum DepSource {
    Path(String),
    Git(String),
}

#[derive(Debug, Clone)]
pub struct DepSpec {
    pub name: String,
    pub source: DepSource,
    /// caret version requirement for git deps (`version = "1.2"` in the
    /// manifest); tags satisfying it are resolved by [`fetch_git_dep`]
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub deps: Vec<DepSpec>,
}

/// Parse the supported TOML subset. Strict on purpose: unknown sections,
/// unknown keys and unquoted values are errors, so typos surface early.
pub fn parse_manifest(src: &str) -> Result<Manifest, String> {
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut deps: Vec<DepSpec> = Vec::new();
    let mut seen_deps: HashSet<String> = HashSet::new();
    let mut section = "";
    for (i, raw) in src.lines().enumerate() {
        let line_no = i + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            let hdr = rest
                .strip_suffix(']')
                .ok_or_else(|| format!("line {line_no}: unterminated section header"))?
                .trim();
            match hdr {
                "package" | "deps" => section = hdr,
                other => {
                    return Err(format!(
                        "line {line_no}: unknown section '[{other}]' (expected [package] or [deps])"
                    ))
                }
            }
            continue;
        }
        let eq = line
            .find('=')
            .ok_or_else(|| format!("line {line_no}: expected 'key = value'"))?;
        let key = line[..eq].trim();
        let val = line[eq + 1..].trim();
        if key.is_empty() {
            return Err(format!("line {line_no}: empty key before '='"));
        }
        match section {
            "package" => match key {
                "name" => {
                    if name.is_some() {
                        return Err(format!("line {line_no}: duplicate key 'name'"));
                    }
                    name = Some(parse_basic_string(val, line_no)?);
                }
                "version" => {
                    if version.is_some() {
                        return Err(format!("line {line_no}: duplicate key 'version'"));
                    }
                    version = Some(parse_basic_string(val, line_no)?);
                }
                _ => {
                    return Err(format!(
                        "line {line_no}: unknown key '{key}' in [package] (expected 'name' or 'version')"
                    ))
                }
            },
            "deps" => {
                if !valid_dep_name(key) {
                    return Err(format!(
                        "line {line_no}: invalid dependency name '{key}' (use letters, digits, '-', '_', '.')"
                    ));
                }
                if !seen_deps.insert(key.to_string()) {
                    return Err(format!("line {line_no}: duplicate dependency '{key}'"));
                }
                let pairs = parse_inline_table(val, line_no)?;
                let mut path: Option<String> = None;
                let mut git: Option<String> = None;
                let mut version: Option<String> = None;
                for (k, v) in pairs {
                    match k.as_str() {
                        "path" => path = Some(v),
                        "git" => git = Some(v),
                        "version" => version = Some(v),
                        _ => {
                            return Err(format!(
                                "line {line_no}: unknown key '{k}' in dependency '{key}' (expected 'path', 'git' or 'version')"
                            ))
                        }
                    }
                }
                let source = match (path, git) {
                    (Some(_), Some(_)) => {
                        return Err(format!(
                            "line {line_no}: dependency '{key}' sets both 'path' and 'git'"
                        ))
                    }
                    (Some(p), None) => DepSource::Path(p),
                    (None, Some(g)) => DepSource::Git(g),
                    (None, None) => {
                        return Err(format!(
                            "line {line_no}: dependency '{key}' needs 'path' or 'git'"
                        ))
                    }
                };
                if matches!(source, DepSource::Path(_)) && version.is_some() {
                    return Err(format!(
                        "line {line_no}: dependency '{key}' is a path dependency — 'version' applies to git dependencies only"
                    ));
                }
                if let Some(r) = &version {
                    if let Err(e) = crate::registry::Req::parse(r) {
                        return Err(format!(
                            "line {line_no}: dependency '{key}': invalid version requirement '{r}': {e}"
                        ));
                    }
                }
                deps.push(DepSpec { name: key.to_string(), source, version });
            }
            _ => {
                return Err(format!(
                    "line {line_no}: expected a [package] or [deps] section header first"
                ))
            }
        }
    }
    let name = name.ok_or("[package] name is required")?;
    // the name becomes the default binary name and a path component, so it
    // follows the same rules as a dependency name (`../x` used to be accepted
    // and produced a binary outside the project directory)
    if !valid_dep_name(&name) || name == "." || name == ".." {
        return Err(format!(
            "invalid package name '{name}' (use letters, digits, '-', '_', '.')"
        ));
    }
    let version = version.unwrap_or_else(|| "0.1.0".into());
    crate::registry::Version::parse(&version)
        .map_err(|e| format!("invalid package version '{version}': {e}"))?;
    Ok(Manifest { name, version, deps })
}

/// drop a trailing `#` comment (outside string literals)
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_str => escaped = true,
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

/// TOML basic string: `"...\"..."` with \n \t \r \\ \" escapes
fn parse_basic_string(v: &str, line_no: usize) -> Result<String, String> {
    if v.len() < 2 || !v.starts_with('"') || !v.ends_with('"') {
        return Err(format!("line {line_no}: expected a quoted string, got '{v}'"));
    }
    let inner = &v[1..v.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => return Err(format!("line {line_no}: unsupported escape '\\{other}'")),
            None => return Err(format!("line {line_no}: dangling escape at end of string")),
        }
    }
    Ok(out)
}

/// `{ key = "value", ... }` — dependencies are declared as inline tables
fn parse_inline_table(v: &str, line_no: usize) -> Result<Vec<(String, String)>, String> {
    let v = v.trim();
    if v.len() < 2 || !v.starts_with('{') || !v.ends_with('}') {
        return Err(format!(
            "line {line_no}: expected an inline table '{{ ... }}', got '{v}'"
        ));
    }
    let inner = &v[1..v.len() - 1];
    // `{ path = "a" } junk = { path = "b" }` starts and ends with braces, so
    // the cheap first/last check above passes; walk the value to prove the
    // *first* table closes exactly at the end
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in v.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_str => escaped = true,
            '"' => in_str = !in_str,
            '{' if !in_str => depth += 1,
            '}' if !in_str => {
                depth = depth.saturating_sub(1);
                if depth == 0 && i + c.len_utf8() != v.len() {
                    return Err(format!(
                        "line {line_no}: trailing text after the inline table: '{}'",
                        v[i + c.len_utf8()..].trim()
                    ));
                }
            }
            _ => {}
        }
    }
    if in_str || depth != 0 {
        return Err(format!("line {line_no}: unterminated inline table '{v}'"));
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    for part in split_top(inner, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (k, val) = part
            .split_once('=')
            .ok_or_else(|| format!("line {line_no}: expected 'key = value' inside '{{ }}'"))?;
        let key = k.trim().to_string();
        if pairs.iter().any(|(prev, _)| *prev == key) {
            return Err(format!(
                "line {line_no}: duplicate key '{key}' inside an inline table"
            ));
        }
        pairs.push((key, parse_basic_string(val.trim(), line_no)?));
    }
    Ok(pairs)
}

/// split on `sep`, ignoring separators inside strings or braces
fn split_top(s: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut escaped = false;
    let mut depth = 0usize;
    for c in s.chars() {
        if escaped {
            escaped = false;
            cur.push(c);
            continue;
        }
        match c {
            '\\' if in_str => {
                escaped = true;
                cur.push(c);
            }
            '"' => {
                in_str = !in_str;
                cur.push(c);
            }
            '{' if !in_str => {
                depth += 1;
                cur.push(c);
            }
            '}' if !in_str => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            c if c == sep && !in_str && depth == 0 => {
                parts.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    parts.push(cur);
    parts
}

/// dependency names become directory names under `.wlel/deps` — no traversal
pub fn valid_dep_name(n: &str) -> bool {
    !n.is_empty()
        && n != "."
        && n != ".."
        && n
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

// ---------------------------------------------------------------------------
// project loading
// ---------------------------------------------------------------------------

/// a loaded project: entry program merged with every dependency
pub struct Project {
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// src/main.wlel (binary) or src/lib.wlel (library-only project)
    pub entry: PathBuf,
    /// true when the entry is src/main.wlel (run/build need this)
    pub is_bin: bool,
    pub program: Program,
    /// non-fatal notes from dependency resolution (e.g. offline tag fallback)
    pub warnings: Vec<String>,
}

/// walk up from `from` until a `wlel.toml` is found
pub fn find_project_dir(from: &Path) -> Result<PathBuf, String> {
    let mut cur = from.to_path_buf();
    loop {
        if cur.join(MANIFEST_FILE).is_file() {
            return Ok(cur);
        }
        if !cur.pop() {
            return Err(format!(
                "no {MANIFEST_FILE} found in {} or any parent directory",
                from.display()
            ));
        }
    }
}

/// load and fully resolve a project at `dir`: entry program + recursive
/// dependencies (path deps resolved relative to their parent project, git
/// deps cloned into the root project's `.wlel/deps/<name>`)
pub fn load_project(dir: &Path) -> Result<Project, String> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let src = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let manifest =
        parse_manifest(&src).map_err(|e| format!("{}: {e}", manifest_path.display()))?;

    let main = dir.join("src/main.wlel");
    let lib = dir.join("src/lib.wlel");
    let (entry, is_bin) = if main.is_file() {
        (main, true)
    } else if lib.is_file() {
        (lib, false)
    } else {
        return Err(format!(
            "project '{}' has no src/main.wlel or src/lib.wlel",
            manifest.name
        ));
    };

    let lock = read_lock(dir);
    let mut resolved: Vec<LockEntry> = Vec::new();
    let mut program = Program::empty();
    let mut visited = HashSet::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut warnings = Vec::new();

    // the entry file's own imports, and its dependency packages, are all
    // modules; load them first, then resolve `use name;` against the complete
    // table (the order of `use` lines never matters)
    let root_prog = load_program(&entry, &mut visited)?;
    merge_into(&mut program, root_prog);
    resolve_deps(
        dir,
        dir,
        &manifest.deps,
        &mut program,
        &mut visited,
        &lock,
        &mut resolved,
        &mut seen,
        0,
        &mut warnings,
    )?;
    bind_named_uses(&mut program);
    modules::check_unique(&program)?;
    finalize_modules(&mut program);

    if !resolved.is_empty() {
        resolved.sort_by(|a, b| a.name.cmp(&b.name));
        write_lock(dir, &resolved);
    }
    Ok(Project { dir: dir.to_path_buf(), manifest, entry, is_bin, program, warnings })
}



/// Rewrite every type annotation, impl target and literal name through the
/// assembled module table.
///
/// Idempotent: runs once when a file tree has been loaded, and again after
/// `use name;` bindings appear (a dependency is only in the table once the
/// whole project has been resolved).
pub fn finalize_modules(program: &mut Program) {
    let exports: HashMap<String, Vec<Export>> =
        program.modules.iter().map(|m| (m.name.clone(), m.exports.clone())).collect();
    let table: HashMap<String, Vec<ImportBinding>> =
        program.modules.iter().map(|m| (m.name.clone(), m.imports.clone())).collect();
    modules::qualify_impl_targets(program, &exports, &table);
    modules::qualify_program_types(program, &exports, &table);
}

/// Resolve `use name;` declarations against the assembled module table.
///
/// A named import has no filesystem path: it refers to a module loaded from a
/// dependency (`use geom;` for `geom = { path = "..." }`) or to a module that
/// another file already pulled in. Running this after every file is loaded is
/// what makes the order of `use` lines irrelevant.
fn bind_named_uses(program: &mut Program) {
    let names: HashMap<String, Vec<Export>> = program
        .modules
        .iter()
        .map(|m| (m.name.clone(), m.exports.clone()))
        .collect();
    let snapshot = program.modules.clone();
    for (mi, m) in snapshot.iter().enumerate() {
        let mut additions: Vec<ImportBinding> = Vec::new();
        for u in &m.uses {
            let Some(named) = &u.named else { continue };
            let Some(exports) = names.get(named) else { continue };
            additions.push(ImportBinding {
                local: u.alias.clone().unwrap_or_else(|| named.clone()),
                target: named.clone(),
                is_module: true,
                explicit: true,
            });
            match &u.names {
                ImportNames::None => {
                    for e in exports {
                        additions.push(ImportBinding {
                            local: e.name.clone(),
                            target: e.qualified.clone(),
                            is_module: false,
                            explicit: false,
                        });
                    }
                }
                ImportNames::All => {
                    for e in exports {
                        additions.push(ImportBinding {
                            local: e.name.clone(),
                            target: e.qualified.clone(),
                            is_module: false,
                            explicit: true,
                        });
                    }
                }
                ImportNames::Some(list) => {
                    for n in list {
                        if let Some(e) = exports.iter().find(|e| e.name == n.name) {
                            additions.push(ImportBinding {
                                local: n.alias.clone().unwrap_or_else(|| n.name.clone()),
                                target: e.qualified.clone(),
                                is_module: false,
                                explicit: true,
                            });
                        }
                    }
                }
            }
        }
        if let Some(target) = program.modules.get_mut(mi) {
            target.imports.extend(additions);
        }
    }
}

fn merge_into(program: &mut Program, sub: Program) {
    program.uses.extend(sub.uses);
    program.structs.extend(sub.structs);
    program.enums.extend(sub.enums);
    program.impls.extend(sub.impls);
    program.funcs.extend(sub.funcs);
    program.tests.extend(sub.tests);
    program.modules.extend(sub.modules);
}

#[allow(clippy::too_many_arguments)]
fn resolve_deps(
    workspace: &Path,
    base: &Path,
    deps: &[DepSpec],
    program: &mut Program,
    visited: &mut HashSet<PathBuf>,
    lock: &[LockEntry],
    resolved: &mut Vec<LockEntry>,
    seen: &mut HashMap<String, String>,
    depth: u32,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if depth > MAX_DEP_DEPTH {
        return Err("dependency nesting too deep (dependency cycle?)".into());
    }
    for dep in deps {
        let dep_root = match &dep.source {
            DepSource::Path(p) => {
                let d = base.join(p);
                if !d.join(MANIFEST_FILE).is_file() {
                    return Err(format!(
                        "path dependency '{}' -> '{}' is not a wlel project (missing {MANIFEST_FILE})",
                        dep.name,
                        d.display()
                    ));
                }
                d
            }
            DepSource::Git(url) => {
                let locked = lock.iter().find(|e| e.name == dep.name);
                let d = git_dep_cache_dir(workspace, &dep.name);
                let gr = fetch_git_dep(url, &d, dep.version.as_deref(), locked, warnings)?;
                resolved.push(LockEntry {
                    name: dep.name.clone(),
                    req: gr.req,
                    rev: gr.rev,
                });
                d
            }
        };

        // dedup/conflict: the same name with the same source is a diamond
        // (resolved once); the same name from a different source is a clash
        let key = match &dep.source {
            DepSource::Path(_) => format!("path:{}", dep_root.display()),
            DepSource::Git(u) => format!("git:{u}"),
        };
        match seen.get(&dep.name) {
            Some(prev) if *prev == key => continue,
            Some(prev) => {
                return Err(format!(
                    "two dependencies share the name '{}' ('{}' vs '{}')",
                    dep.name,
                    prev.trim_start_matches("git:").trim_start_matches("path:"),
                    key.trim_start_matches("git:").trim_start_matches("path:")
                ))
            }
            None => {
                seen.insert(dep.name.clone(), key);
            }
        }

        let mf_path = dep_root.join(MANIFEST_FILE);
        let mf_src = fs::read_to_string(&mf_path)
            .map_err(|e| format!("cannot read {}: {e}", mf_path.display()))?;
        let mf = parse_manifest(&mf_src)
            .map_err(|e| format!("{}: {e}", mf_path.display()))?;
        if mf.name != dep.name {
            return Err(format!(
                "dependency '{}' declares name '{}' in its wlel.toml",
                dep.name, mf.name
            ));
        }
        let lib = dep_root.join("src/lib.wlel");
        if !lib.is_file() {
            return Err(format!(
                "dependency '{}' ({}) has no src/lib.wlel",
                dep.name,
                dep_root.display()
            ));
        }
        // nested dependencies first (their definitions must exist before the
        // parent's are merged — order does not matter to the checker, but a
        // failing nested manifest should be reported before anything else)
        resolve_deps(workspace, &dep_root, &mf.deps, program, visited, lock, resolved, seen, depth + 1, warnings)?;
        // a dependency is a module, named by its package (`use geom;` or
        // `geom::Vec2`). Loading it as a non-root file is what qualifies its
        // declarations.
        let sub = load_program_with(&lib, visited, false, Some(&dep.name))?;
        merge_into(program, sub);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// loader (shared with single-file mode)
// ---------------------------------------------------------------------------


/// The scope entries a single `use` creates in the importing module:
/// the module binding itself (`geom::add`) plus, for `{ a, b }` / `{ * }`,
/// each imported name.
pub fn scope_bindings(
    u: &UseDecl,
    sub_module: &str,
    bound: &str,
    sub: Option<&ModuleInfo>,
) -> Result<Vec<ImportBinding>, String> {
    let mut out = vec![ImportBinding {
        local: bound.to_string(),
        target: sub_module.to_string(),
        is_module: true,
        explicit: true,
    }];
    let exports: Vec<Export> = sub.map(|m| m.exports.clone()).unwrap_or_default();
    match &u.names {
        ImportNames::None => {
            // `use "lib/geom.wlel";` keeps the pre-module behaviour: the
            // module's names are reachable without a qualifier, but an
            // explicit `{ ... }` list wins if two modules collide
            for e in exports {
                out.push(ImportBinding {
                    local: e.name.clone(),
                    target: e.qualified.clone(),
                    is_module: false,
                    explicit: false,
                });
            }
        }
        ImportNames::All => {
            for e in exports {
                out.push(ImportBinding {
                    local: e.name.clone(),
                    target: e.qualified.clone(),
                    is_module: false,
                    explicit: true,
                });
            }
        }
        ImportNames::Some(list) => {
            for n in list {
                let Some(e) = exports.iter().find(|e| e.name == n.name) else {
                    return Err(format!(
                        "{}: module '{sub_module}' has no item '{}'",
                        n.span, n.name
                    ));
                };
                out.push(ImportBinding {
                    local: n.alias.clone().unwrap_or_else(|| n.name.clone()),
                    target: e.qualified.clone(),
                    is_module: false,
                    explicit: true,
                });
            }
        }
    }
    Ok(out)
}

/// lex + parse a `.wlel` file and recursively merge its `use "path.wlel"` imports
/// (relative to the importing file, diamond-deduped). Returns the merged
/// program; every function/struct/test carries the absolute path of the file
/// it was parsed from.
pub fn load_program(path: &Path, visited: &mut HashSet<PathBuf>) -> Result<Program, String> {
    let mut program = load_program_inner(path, visited, true)?;
    bind_named_uses(&mut program);
    modules::check_unique(&program)?;
    finalize_modules(&mut program);
    Ok(program)
}

/// `is_root` marks the file the user asked for: its items stay unqualified so
/// `main` remains the C entry point. Every imported file is qualified with its
/// module name.
pub fn load_program_inner(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    is_root: bool,
) -> Result<Program, String> {
    load_program_with(path, visited, is_root, None)
}

/// As [`load_program_inner`], with an explicit module name.
///
/// A package dependency is named by its `wlel.toml` `[package] name`, not by
/// its file stem, so `use geom;` and `geom::Pt` mean the package.
pub fn load_program_with(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    is_root: bool,
    module_override: Option<&str>,
) -> Result<Program, String> {
    let canonical =
        path.canonicalize().map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !visited.insert(canonical.clone()) {
        // already merged (diamond import)
        return Ok(Program::empty());
    }
    let src = fs::read_to_string(&canonical)
        .map_err(|e| format!("cannot read {}: {e}", canonical.display()))?;
    let toks = Lexer::new(&src)
        .tokenize()
        // LexError's Display already carries `line:col`; prefixing it again
        // produced `file:1:5: 1:5: message`
        .map_err(|e| format!("{}:{e}", canonical.display()))?;
    let (mut program, parse_errors) = Parser::new(&toks).program();
    if !parse_errors.is_empty() {
        // report every syntax error found, not just the first
        let msg = parse_errors
            .iter()
            .map(|e| format!("{}: syntax error: {e}", canonical.display()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(msg);
    }

    // remember where each function/struct came from, for `#line` diagnostics
    // and unused-import detection
    let file_str = canonical.display().to_string();
    for f in program.funcs.iter_mut() {
        f.file = file_str.clone();
    }
    for s in program.structs.iter_mut() {
        s.file = file_str.clone();
    }
    for e in program.enums.iter_mut() {
        e.file = file_str.clone();
    }
    for i in program.impls.iter_mut() {
        i.file = file_str.clone();
    }
    for t in program.tests.iter_mut() {
        t.file = file_str.clone();
    }

    // The module this file belongs to. The root module keeps the empty name:
    // its items are the program's own and must stay unqualified.
    let declared = program.module.clone();
    let stem = canonical
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let module = if is_root {
        String::new()
    } else if let Some(forced) = module_override {
        forced.to_string()
    } else if declared.is_empty() {
        stem
    } else {
        declared
    };
    // record the exports in source terms *before* qualifying, so the table
    // maps `add` -> `geom::add`
    let raw_exports = modules::exports_of(&program);
    modules::qualify(&mut program, &module, is_root);
    modules::tag_module(&mut program, &module);
    let mut own_module = ModuleInfo {
        name: module.clone(),
        file: file_str.clone(),
        exports: Vec::new(),
        imports: Vec::new(),
        uses: program.uses.clone(),
    };
    if !is_root {
        // rename the export table to the qualified names
        own_module.exports = raw_exports
            .into_iter()
            .map(|e| Export {
                qualified: format!("{module}::{}", e.name),
                owner: if e.owner.is_empty() {
                    String::new()
                } else {
                    format!("{module}::{}", e.owner)
                },
                ..e
            })
            .collect();
    } else {
        own_module.exports = raw_exports;
    }

    // recursively merge `use "path.wlel";` imports (relative to the importing file)
    let base_dir =
        canonical.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let mut import_resolved: Vec<Option<String>> = Vec::new();
    let mut modules: Vec<ModuleInfo> = vec![own_module];
    let mut own_imports: Vec<ImportBinding> = Vec::new();
    // `use std` in any imported file must also pull in the embedded library,
    // otherwise a dependency that uses std breaks programs that never import
    // it themselves — propagate std imports up the whole import tree
    let mut propagated_std_uses: Vec<UseDecl> = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut impls = Vec::new();
    let mut funcs = Vec::new();
    let mut tests = Vec::new();
    for u in &program.uses {
        let Some(rel) = &u.path else {
            import_resolved.push(None);
            continue;
        };
        let ip = base_dir.join(rel);
        let canonical = ip.canonicalize().ok().map(|p| p.display().to_string());
        // A diamond import returns an empty program (the file was merged
        // already), so the module it names has to come from the table.
        let mut sub = load_program_inner(&ip, visited, false)?;
        let declared = sub.module.clone();
        let info = sub
            .modules
            .iter()
            .find(|m| m.name == declared)
            .cloned()
            .or_else(|| {
                canonical
                    .as_ref()
                    .and_then(|c| modules.iter().find(|m| &m.file == c).cloned())
            });
        // the name the import is reached by: what the file declares, else its
        // stem — an `as` alias wins when present
        let sub_module = match &info {
            Some(m) => m.name.clone(),
            None => modules::bind_name(&declared, rel),
        };
        let bound = u.alias.clone().unwrap_or_else(|| sub_module.clone());
        // keep the canonical path so the checker can attribute usage
        import_resolved.push(canonical);

        // every name this import brings into the importing module's scope
        own_imports.extend(scope_bindings(u, &sub_module, &bound, info.as_ref())?);
        modules.append(&mut sub.modules);
        for su in sub.uses {
            if su.path.is_none() {
                propagated_std_uses.push(su);
            }
        }
        structs.extend(sub.structs);
        enums.extend(sub.enums);
        impls.extend(sub.impls);
        funcs.extend(sub.funcs);
        tests.extend(sub.tests);
    }
    for (u, resolved) in program.uses.iter_mut().zip(import_resolved) {
        u.resolved = resolved;
    }
    program.modules = modules;
    program.modules[0].imports = own_imports;

    finalize_modules(&mut program);
    program.uses.extend(propagated_std_uses);
    structs.append(&mut program.structs);
    enums.append(&mut program.enums);
    impls.append(&mut program.impls);
    funcs.append(&mut program.funcs);
    tests.append(&mut program.tests);
    program.structs = structs;
    program.enums = enums;
    program.impls = impls;
    program.funcs = funcs;
    program.tests = tests;
    Ok(program)
}

// ---------------------------------------------------------------------------
// lockfile (wlel.lock)
// ---------------------------------------------------------------------------

/// one resolved git dependency: the pinned revision plus the version
/// requirement that produced it (`wlel.lock` records `"req@rev"`, or plain
/// `"rev"` for requirement-less deps — the pre-registry format)
#[derive(Debug, Clone)]
pub struct LockEntry {
    pub name: String,
    pub req: Option<String>,
    pub rev: String,
}

/// `name = "revision"` (or `name = "req@revision"`) lines; comments and
/// blanks ignored
pub fn read_lock(dir: &Path) -> Vec<LockEntry> {
    let mut out = Vec::new();
    let Ok(src) = fs::read_to_string(dir.join(LOCK_FILE)) else {
        return out;
    };
    for line in src.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let k = k.trim();
            let v = v.trim().trim_matches('"');
            if k.is_empty() || v.is_empty() {
                continue;
            }
            let (req, rev) = match v.split_once('@') {
                Some((r, rev)) if !r.is_empty() && !rev.is_empty() => {
                    (Some(r.to_string()), rev.to_string())
                }
                _ => (None, v.to_string()),
            };
            out.push(LockEntry { name: k.to_string(), req, rev });
        }
    }
    out
}

pub fn write_lock(dir: &Path, entries: &[LockEntry]) {
    let mut body =
        String::from("# wlel.lock - resolved git dependency revisions (managed by wlel; commit this file)\n");
    for e in entries {
        let val = match &e.req {
            Some(r) => format!("{r}@{}", e.rev),
            None => e.rev.clone(),
        };
        body.push_str(&format!("{} = \"{val}\"\n", e.name));
    }
    let _ = fs::write(dir.join(LOCK_FILE), body);
}

/// Remove the `name = { ... }` line from the `[deps]` section, leaving every
/// other byte alone (comments, ordering, the `[package]` block). Returns
/// `None` when the dependency is not declared. The `[deps]` header stays even
/// when it becomes empty — deleting it would silently change the file's shape.
pub fn remove_dep_line(src: &str, name: &str) -> Option<String> {
    let raw: Vec<&str> = src.lines().collect();
    let h = raw.iter().position(|l| l.trim() == "[deps]")?;
    let end = raw[h + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map(|i| h + 1 + i)
        .unwrap_or(raw.len());
    let mut hit: Option<usize> = None;
    for (i, l) in raw.iter().enumerate().take(end).skip(h + 1) {
        let t = l.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some((k, _)) = t.split_once('=') {
            if k.trim() == name {
                hit = Some(i);
                break;
            }
        }
    }
    let hit = hit?;
    let mut out: Vec<&str> = Vec::with_capacity(raw.len());
    for (i, l) in raw.iter().enumerate() {
        if i == hit {
            continue;
        }
        out.push(l);
    }
    // collapse the blank line the removal may have left between two entries
    let mut collapsed: Vec<&str> = Vec::with_capacity(out.len());
    for l in out {
        if l.trim().is_empty()
            && collapsed.last().map(|p: &&str| p.trim().is_empty()).unwrap_or(false)
        {
            continue;
        }
        collapsed.push(l);
    }
    let mut s = collapsed.join("\n");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

/// Read the manifest of `dir`, or a message naming the file.
pub fn read_manifest(dir: &Path) -> Result<(String, Manifest), String> {
    let p = dir.join(MANIFEST_FILE);
    let src = fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    let mf = parse_manifest(&src).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((src, mf))
}

// ---------------------------------------------------------------------------
// git dependencies
// ---------------------------------------------------------------------------

fn run_git(mut cmd: Command, what: &str) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("wlel: cannot run git: {e} (is git installed?)"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("wlel: git {what} failed:\n{}", err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_head(dir: &Path) -> Result<String, String> {
    run_git(
        {
            let mut c = Command::new("git");
            c.arg("-C").arg(dir).arg("rev-parse").arg("HEAD");
            c
        },
        "rev-parse HEAD",
    )
}

/// the dependency cache clone for `name` under the root project
pub fn git_dep_cache_dir(root: &Path, name: &str) -> PathBuf {
    root.join(DEPS_DIR).join(name)
}

/// the version the dependency declares in its own `wlel.toml` (at whatever
/// revision is currently checked out)
fn dep_manifest_version(dir: &Path) -> Result<crate::registry::Version, String> {
    let p = dir.join(MANIFEST_FILE);
    let src =
        fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    let mf = parse_manifest(&src).map_err(|e| format!("{}: {e}", p.display()))?;
    crate::registry::Version::parse(&mf.version)
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// best git tag satisfying `req`, highest version wins (`v`-prefixed and
/// bare tags both count); the error lists what the repo actually has
fn best_tag(dir: &Path, req: &crate::registry::Req) -> Result<(String, crate::registry::Version), String> {
    let out = run_git(
        {
            let mut c = Command::new("git");
            c.arg("-C").arg(dir).arg("tag").arg("-l");
            c
        },
        "tag -l",
    )?;
    let mut best: Option<(String, crate::registry::Version)> = None;
    let mut all: Vec<&str> = Vec::new();
    for t in out.lines() {
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        all.push(t);
        if let Ok(v) = crate::registry::Version::parse(t) {
            if req.matches(&v) && best.as_ref().is_none_or(|(_, bv)| v > *bv) {
                best = Some((t.to_string(), v));
            }
        }
    }
    best.ok_or_else(|| {
        format!(
            "no git tag satisfies requirement '{req}' (tags found: {})",
            if all.is_empty() { "none".to_string() } else { all.join(", ") }
        )
    })
}

/// best-effort tag refresh: fails softly (recorded as a warning) so a warm
/// clone keeps working offline
fn refresh_tags(url: &str, dir: &Path, warnings: &mut Vec<String>) {
    let r = run_git(
        {
            let mut c = Command::new("git");
            c.arg("-C").arg(dir).arg("fetch").arg("--all");
            c
        },
        "fetch",
    );
    if let Err(e) = r {
        warnings.push(format!(
            "git fetch for '{url}' failed (offline?); resolving from the already-fetched tags ({e})"
        ));
    }
}

pub struct GitResolve {
    pub rev: String,
    /// tag name when the revision came from a version requirement
    pub tag: Option<String>,
    /// the requirement the lock file should record (None = pin HEAD)
    pub req: Option<String>,
}

/// clone `url` into `dir` (or reuse/repair an existing clone) and return the
/// resolved revision.
///
/// With a version requirement the revision is the highest tag satisfying it
/// (caret semantics), verified against the dependency's own `wlel.toml` —
/// the tag and the declared version must agree. Without one the clone's
/// HEAD is pinned, exactly like the pre-registry behavior.
///
/// A matching `locked` entry short-circuits: the pinned revision is checked
/// out locally (no network) and only re-resolved when its version no longer
/// satisfies the requirement or the recorded requirement drifted — that is
/// what makes builds reproducible and offline-friendly.
pub fn fetch_git_dep(
    url: &str,
    dir: &Path,
    req: Option<&str>,
    locked: Option<&LockEntry>,
    warnings: &mut Vec<String>,
) -> Result<GitResolve, String> {
    // a manifest is untrusted input: a "URL" starting with `-` would be
    // parsed by git as an option (`--upload-pack=...`), not a repository
    if url.starts_with('-') || url.is_empty() {
        return Err(format!(
            "wlel: git dependency URL must not start with '-': '{url}'"
        ));
    }
    let checkout = |dir: &Path, rev: &str| -> Result<(), String> {
        if rev.starts_with('-') || rev.is_empty() {
            return Err(format!("wlel: refusing to check out '{rev}'"));
        }
        run_git(
            {
                let mut c = Command::new("git");
                c.arg("-C").arg(dir).arg("checkout").arg("--detach").arg(rev);
                c
            },
            &format!("checkout {rev}"),
        )
        .map(|_| ())
    };
    let mut have_clone = dir.join(".git").exists();
    if have_clone {
        // a clone from a different URL must not satisfy this dep (the name
        // was re-pointed, or an earlier `wlel add` cached another remote) —
        // wipe and re-clone
        let origin = run_git(
            {
                let mut c = Command::new("git");
                c.arg("-C").arg(dir).arg("remote").arg("get-url").arg("origin");
                c
            },
            "remote get-url origin",
        );
        if matches!(origin, Ok(o) if o != url) {
            let _ = fs::remove_dir_all(dir);
            have_clone = false;
        }
    }
    if !have_clone {
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        run_git(
            {
                let mut c = Command::new("git");
                c.arg("clone").arg("--").arg(url).arg(dir);
                c
            },
            &format!("clone {url}"),
        )?;
    }

    // 1) reuse the pin when the requirement did not drift and the pinned
    //    revision still satisfies it
    if let Some(l) = locked {
        let pin_matches = match (req, &l.req) {
            (None, _) => true,
            (Some(r), Some(lr)) => lr == r,
            (Some(_), None) => false,
        };
        if pin_matches {
            if git_head(dir)? != l.rev {
                refresh_tags(url, dir, warnings);
                checkout(dir, &l.rev)?;
            }
            let ok = match req {
                None => true,
                Some(r) => match crate::registry::Req::parse(r) {
                    Ok(rq) => match dep_manifest_version(dir) {
                        Ok(v) => rq.matches(&v),
                        Err(_) => false,
                    },
                    Err(_) => false,
                },
            };
            if ok {
                return Ok(GitResolve { rev: l.rev.clone(), tag: None, req: l.req.clone() });
            }
            // otherwise: fall through and re-resolve
        }
    }

    // 2) fresh resolution
    let mut tag = None;
    if let Some(r) = req {
        let rq = crate::registry::Req::parse(r)
            .map_err(|e| format!("invalid version requirement '{r}': {e}"))?;
        if have_clone {
            refresh_tags(url, dir, warnings);
        }
        let (t, _) = best_tag(dir, &rq)?;
        checkout(dir, &t)?;
        tag = Some(t);
    }
    let rev = git_head(dir)?;
    if let Some(r) = req {
        let rq = crate::registry::Req::parse(r)
            .map_err(|e| format!("invalid version requirement '{r}': {e}"))?;
        let v = dep_manifest_version(dir)?;
        if !rq.matches(&v) {
            return Err(format!(
                "dependency declares version {} which does not satisfy requirement '{r}'",
                v
            ));
        }
    }
    Ok(GitResolve { rev, tag, req: req.map(|s| s.to_string()) })
}

// ---------------------------------------------------------------------------
// dependency tree
// ---------------------------------------------------------------------------

/// one node of `wlel tree`
#[derive(Debug, Clone)]
pub struct DepNode {
    /// "name v1.2.3", or "name (path ../lib)" / "name (git URL @ rev)"
    pub label: String,
    pub children: Vec<DepNode>,
}

/// Render the dependency graph rooted at `dir`. Path dependencies are read
/// from disk, git dependencies from the `.wlel/deps` clone (so the revision
/// shown is the one that will actually compile). Cycles and missing clones
/// are reported inline instead of failing: `tree` is a diagnostic tool.
pub fn dependency_tree(dir: &Path) -> Result<DepNode, String> {
    let (_, root) = read_manifest(dir)?;
    let lock = read_lock(dir);
    let rev_of = |name: &str| {
        lock.iter()
            .find(|e| e.name == name)
            .map(|e| e.rev.chars().take(10).collect::<String>())
    };
    let mut visited: HashSet<PathBuf> = HashSet::new();
    visited.insert(dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()));
    let children = tree_children(dir, &root, &rev_of, &mut visited, 0)?;
    Ok(DepNode {
        label: format!("{} v{}", root.name, root.version),
        children,
    })
}

fn tree_children(
    root: &Path,
    mf: &Manifest,
    rev_of: &dyn Fn(&str) -> Option<String>,
    visited: &mut HashSet<PathBuf>,
    depth: u32,
) -> Result<Vec<DepNode>, String> {
    let mut out = Vec::new();
    for dep in &mf.deps {
        let (dep_dir, source) = match &dep.source {
            DepSource::Path(p) => (root.join(p), format!("path {p}")),
            DepSource::Git(url) => (
                git_dep_cache_dir(root, &dep.name),
                match rev_of(&dep.name) {
                    Some(rev) => format!("git {url} @ {rev}"),
                    None => format!("git {url} (not resolved yet)"),
                },
            ),
        };
        if depth + 1 > MAX_DEP_DEPTH {
            out.push(DepNode {
                label: format!("{} ({source}) — depth limit reached", dep.name),
                children: Vec::new(),
            });
            continue;
        }
        let canonical = dep_dir.canonicalize().unwrap_or_else(|_| dep_dir.clone());
        if !visited.insert(canonical.clone()) {
            out.push(DepNode {
                label: format!("{} ({source}) — already shown", dep.name),
                children: Vec::new(),
            });
            continue;
        }
        let Ok((_, sub)) = read_manifest(&dep_dir) else {
            out.push(DepNode {
                label: format!(
                    "{} ({source}) — not resolved (run `wlel build`)",
                    dep.name
                ),
                children: Vec::new(),
            });
            continue;
        };
        let children = tree_children(root, &sub, rev_of, visited, depth + 1)?;
        out.push(DepNode {
            label: format!("{} v{} ({source})", sub.name, sub.version),
            children,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// update
// ---------------------------------------------------------------------------

/// Re-resolve the git dependency pins (`wlel update [name]`): drop the lock
/// entries, resolve again (fetching tags), and report every revision that
/// moved. Path dependencies are not pinned and are left alone. On failure the
/// previous lock is restored.
pub fn update_dependencies(dir: &Path, only: Option<&str>) -> Result<Vec<String>, String> {
    let (_, mf) = read_manifest(dir)?;
    if let Some(name) = only {
        if !mf.deps.iter().any(|d| d.name == name) {
            return Err(format!("'{name}' is not a dependency of '{}'", mf.name));
        }
        if !mf
            .deps
            .iter()
            .any(|d| d.name == name && matches!(d.source, DepSource::Git(_)))
        {
            return Ok(vec![format!("'{name}' is a path dependency — nothing to update")]);
        }
    }
    let before = read_lock(dir);
    let mut reduced = before.clone();
    match only {
        Some(name) => reduced.retain(|e| e.name != name),
        None => reduced.clear(),
    }
    write_lock(dir, &reduced);
    let proj = match load_project(dir) {
        Ok(p) => p,
        Err(e) => {
            write_lock(dir, &before);
            return Err(e);
        }
    };
    for w in &proj.warnings {
        // resolution notes (offline fallbacks) go to the caller as-is
        let _ = w;
    }
    let after = read_lock(dir);
    let mut report = Vec::new();
    for dep in &mf.deps {
        if !matches!(dep.source, DepSource::Git(_)) {
            continue;
        }
        if let Some(name) = only {
            if dep.name != name {
                continue;
            }
        }
        let old = before.iter().find(|e| e.name == dep.name);
        let new = after.iter().find(|e| e.name == dep.name);
        let short = |r: &str| r.chars().take(10).collect::<String>();
        match (old, new) {
            (Some(o), Some(n)) if o.rev != n.rev => {
                report.push(format!("updated '{}' {} -> {}", dep.name, short(&o.rev), short(&n.rev)));
            }
            (Some(o), Some(n)) => {
                report.push(format!("up to date '{}' ({})", dep.name, short(&o.rev)));
                let _ = n;
            }
            (None, Some(n)) => report.push(format!("resolved '{}' ({})", dep.name, short(&n.rev))),
            (Some(_), None) => report.push(format!("dropped '{}' from wlel.lock", dep.name)),
            (None, None) => {}
        }
    }
    if report.is_empty() {
        report.push("no git dependencies to update".into());
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// clean
// ---------------------------------------------------------------------------

/// Remove the generated dependency clones (`.wlel/`). Returns the number of
/// cached clones removed. `wlel.lock` is kept — it is meant to be committed.
pub fn clean_project(dir: &Path) -> Result<usize, String> {
    let deps = dir.join(DEPS_DIR);
    let n = fs::read_dir(&deps)
        .map(|rd| rd.filter_map(|e| e.ok()).count())
        .unwrap_or(0);
    let cache = dir.join(".wlel");
    if cache.exists() {
        fs::remove_dir_all(&cache)
            .map_err(|e| format!("cannot remove {}: {e}", cache.display()))?;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// scaffolding (wlel new)
// ---------------------------------------------------------------------------

/// the files `wlel new` / `wlel init` write. `init` keeps an existing
/// `src/main.wlel`, `.gitignore` and README; `new` writes everything into a
/// fresh (empty) directory.
fn project_files(name: &str) -> Vec<(&'static str, String)> {
    let toml = format!(
        "# {MANIFEST_FILE} - project manifest\n\
         [package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         \n\
         # Dependencies:\n\
         # [deps]\n\
         # mylib  = {{ path = \"../mylib\" }}\n\
         # json   = {{ git = \"https://github.com/user/wlel-json\", version = \"1.0\" }}\n"
    );
    let main_wl = "use std;\n\
                   \n\
                   fn main() -> int {\n\
                   \x20   std::println_str(\"hello, wlel!\");\n\
                   \x20   return 0;\n\
                   }\n\
                   \n\
                   test \"sanity\" {\n\
                   \x20   assert_eq(2 + 2, 4);\n\
                   }\n";
    let readme = format!(
        "# {name}\n\
         \n\
         A Wlel project.\n\
         \n\
         ```bash\n\
         wlel run      # run src/main.wlel (safe mode: bounds and div-zero checks)\n\
         wlel test     # run every `test \"name\" {{ ... }}` block\n\
         wlel check    # type-check only\n\
         wlel build    # release binary named `{name}`\n\
         wlel fmt      # canonical formatting\n\
         ```\n\
         \n\
         Requires `wlel` 1.0+ and a C compiler (`cc`, `gcc` or `clang`).\n\
         `wlel.lock` pins git dependency revisions — commit it. `.wlel/` is a\n\
         generated clone cache — ignore it.\n"
    );
    // the project CI installs the exact compiler release that scaffolded the
    // project, so a green build is reproducible
    let version = env!("CARGO_PKG_VERSION");
    let ci = format!(
        "name: CI\n\
         on: [push, pull_request]\n\
         \n\
         jobs:\n\
         \x20 test:\n\
         \x20   runs-on: ubuntu-latest\n\
         \x20   steps:\n\
         \x20     - uses: actions/checkout@v4\n\
         \x20     - name: Install wlel {version}\n\
         \x20       run: |\n\
         \x20         url=https://github.com/WildanDeveloper/wlel/releases/download/v{version}/wlel-{version}-x86_64-unknown-linux-gnu.tar.gz\n\
         \x20         curl -fsSL \"$url\" | tar -xz\n\
         \x20         sudo mv wlel-{version}-x86_64-unknown-linux-gnu/wlel /usr/local/bin/\n\
         \x20         wlel --version\n\
         \x20     - name: Type-check\n\
         \x20       run: wlel check\n\
         \x20     - name: Tests\n\
         \x20       run: wlel test\n\
         \x20     - name: Formatting\n\
         \x20       run: wlel fmt -check\n"
    );
    let gitignore = format!("/{name}\n/{DEPS_DIR}/\n");
    vec![
        (MANIFEST_FILE, toml),
        ("src/main.wlel", main_wl.into()),
        ("README.md", readme),
        (".github/workflows/ci.yml", ci),
        (".gitignore", gitignore),
    ]
}

/// create a minimal runnable project at `dir` (`wlel new <dir>`)
pub fn scaffold(dir: &Path) -> Result<PathBuf, String> {
    if dir.exists() {
        let entries =
            fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        if entries.count() > 0 {
            return Err(format!("'{}' is not empty", dir.display()));
        }
    }
    let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err("invalid project directory".into());
    };
    if !valid_dep_name(&name) {
        return Err(format!(
            "invalid project name '{name}' (use letters, digits, '-', '_', '.')"
        ));
    }
    fs::create_dir_all(dir.join("src"))
        .map_err(|e| format!("cannot create {}: {e}", dir.join("src").display()))?;
    for (rel, content) in project_files(&name) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        fs::write(&p, content).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
    }
    Ok(dir.to_path_buf())
}

/// `wlel init [dir]`: adopt an existing directory. Writes the manifest and the
/// project metadata, and only creates `src/main.wlel` when there is no entry
/// point yet, so it is safe on a directory that already has sources.
pub fn init_project(dir: &Path) -> Result<PathBuf, String> {
    if dir.join(MANIFEST_FILE).exists() {
        return Err(format!("{} already has a {MANIFEST_FILE}", dir.display()));
    }
    let name = dir
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .ok_or("invalid project directory")?;
    if !valid_dep_name(&name) {
        return Err(format!(
            "invalid project name '{name}' (use letters, digits, '-', '_', '.')"
        ));
    }
    fs::create_dir_all(dir.join("src"))
        .map_err(|e| format!("cannot create {}: {e}", dir.join("src").display()))?;
    let has_main = dir.join("src/main.wlel").exists() || dir.join("src/lib.wlel").exists();
    for (rel, content) in project_files(&name) {
        let p = dir.join(rel);
        if p.exists() {
            continue; // never overwrite existing work
        }
        if rel == "src/main.wlel" && has_main {
            continue; // the project already has an entry point
        }
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        fs::write(&p, content).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
    }
    Ok(dir.to_path_buf())
}
