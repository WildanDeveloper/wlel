//! Project mode: `wlel.toml` manifests, `wlel new` scaffolding, dependency
//! resolution (path + git), and the `wlel.lock` revision pin file.
//!
//! Layout conventions for a project directory:
//!
//! ```text
//! myapp/
//!   wlel.toml      [package] name/version, optional [deps]
//!   src/main.wl    binary entry (required for `run`/`build`)
//!   src/lib.wl     library entry (`check`/`test` work on library-only
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
//! rejected.

use crate::ast::{Program, UseDecl};
use crate::lexer::Lexer;
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
                for (k, v) in pairs {
                    match k.as_str() {
                        "path" => path = Some(v),
                        "git" => git = Some(v),
                        _ => {
                            return Err(format!(
                                "line {line_no}: unknown key '{k}' in dependency '{key}' (expected 'path' or 'git')"
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
                deps.push(DepSpec { name: key.to_string(), source });
            }
            _ => {
                return Err(format!(
                    "line {line_no}: expected a [package] or [deps] section header first"
                ))
            }
        }
    }
    let name = name.ok_or("[package] name is required")?;
    Ok(Manifest {
        name,
        version: version.unwrap_or_else(|| "0.1.0".into()),
        deps,
    })
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
    let mut pairs = Vec::new();
    for part in split_top(inner, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (k, val) = part
            .split_once('=')
            .ok_or_else(|| format!("line {line_no}: expected 'key = value' inside '{{ }}'"))?;
        pairs.push((k.trim().to_string(), parse_basic_string(val.trim(), line_no)?));
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
fn valid_dep_name(n: &str) -> bool {
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
    /// src/main.wl (binary) or src/lib.wl (library-only project)
    pub entry: PathBuf,
    /// true when the entry is src/main.wl (run/build need this)
    pub is_bin: bool,
    pub program: Program,
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

    let main = dir.join("src/main.wl");
    let lib = dir.join("src/lib.wl");
    let (entry, is_bin) = if main.is_file() {
        (main, true)
    } else if lib.is_file() {
        (lib, false)
    } else {
        return Err(format!(
            "project '{}' has no src/main.wl or src/lib.wl",
            manifest.name
        ));
    };

    let lock = read_lock(dir);
    let mut resolved: Vec<(String, String)> = Vec::new();
    let mut program = Program { uses: Vec::new(), structs: Vec::new(), enums: Vec::new(), funcs: Vec::new(), tests: Vec::new() };
    let mut visited = HashSet::new();
    let mut seen: HashMap<String, String> = HashMap::new();

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
    )?;

    if !resolved.is_empty() {
        resolved.sort();
        write_lock(dir, &resolved);
    }
    Ok(Project { dir: dir.to_path_buf(), manifest, entry, is_bin, program })
}

fn merge_into(program: &mut Program, sub: Program) {
    program.uses.extend(sub.uses);
    program.structs.extend(sub.structs);
    program.enums.extend(sub.enums);
    program.funcs.extend(sub.funcs);
    program.tests.extend(sub.tests);
}

#[allow(clippy::too_many_arguments)]
fn resolve_deps(
    workspace: &Path,
    base: &Path,
    deps: &[DepSpec],
    program: &mut Program,
    visited: &mut HashSet<PathBuf>,
    lock: &[(String, String)],
    resolved: &mut Vec<(String, String)>,
    seen: &mut HashMap<String, String>,
    depth: u32,
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
                let locked = lock
                    .iter()
                    .find(|(n, _)| n == &dep.name)
                    .map(|(_, c)| c.as_str());
                let d = workspace.join(DEPS_DIR).join(&dep.name);
                let commit = fetch_git_dep(url, &d, locked)?;
                resolved.push((dep.name.clone(), commit));
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
        let lib = dep_root.join("src/lib.wl");
        if !lib.is_file() {
            return Err(format!(
                "dependency '{}' ({}) has no src/lib.wl",
                dep.name,
                dep_root.display()
            ));
        }
        // nested dependencies first (their definitions must exist before the
        // parent's are merged — order does not matter to the checker, but a
        // failing nested manifest should be reported before anything else)
        resolve_deps(workspace, &dep_root, &mf.deps, program, visited, lock, resolved, seen, depth + 1)?;
        let sub = load_program(&lib, visited)?;
        merge_into(program, sub);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// loader (shared with single-file mode)
// ---------------------------------------------------------------------------

/// lex + parse a `.wl` file and recursively merge its `use "path.wl"` imports
/// (relative to the importing file, diamond-deduped). Returns the merged
/// program; every function/struct/test carries the absolute path of the file
/// it was parsed from.
pub fn load_program(path: &Path, visited: &mut HashSet<PathBuf>) -> Result<Program, String> {
    let canonical =
        path.canonicalize().map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !visited.insert(canonical.clone()) {
        // already merged (diamond import)
        return Ok(Program {
            uses: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            funcs: Vec::new(),
            tests: Vec::new(),
        });
    }
    let src = fs::read_to_string(&canonical)
        .map_err(|e| format!("cannot read {}: {e}", canonical.display()))?;
    let toks = Lexer::new(&src)
        .tokenize()
        .map_err(|e| format!("{}:{}:{}: {e}", canonical.display(), e.line, e.col))?;
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
    for t in program.tests.iter_mut() {
        t.file = file_str.clone();
    }

    // recursively merge `use "path.wl";` imports (relative to the importing file)
    let base_dir =
        canonical.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let mut import_resolved: Vec<Option<String>> = Vec::new();
    // `use std` in any imported file must also pull in the embedded library,
    // otherwise a dependency that uses std breaks programs that never import
    // it themselves — propagate std imports up the whole import tree
    let mut propagated_std_uses: Vec<UseDecl> = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut funcs = Vec::new();
    let mut tests = Vec::new();
    for u in &program.uses {
        let Some(rel) = &u.path else {
            import_resolved.push(None);
            continue;
        };
        let ip = base_dir.join(rel);
        let sub = load_program(&ip, visited)?;
        // keep the canonical path so the checker can attribute usage
        import_resolved.push(ip.canonicalize().ok().map(|p| p.display().to_string()));
        for su in sub.uses {
            if su.path.is_none() {
                propagated_std_uses.push(su);
            }
        }
        structs.extend(sub.structs);
        enums.extend(sub.enums);
        funcs.extend(sub.funcs);
        tests.extend(sub.tests);
    }
    for (u, resolved) in program.uses.iter_mut().zip(import_resolved) {
        u.resolved = resolved;
    }
    program.uses.extend(propagated_std_uses);
    structs.append(&mut program.structs);
    enums.append(&mut program.enums);
    funcs.append(&mut program.funcs);
    tests.append(&mut program.tests);
    program.structs = structs;
    program.enums = enums;
    program.funcs = funcs;
    program.tests = tests;
    Ok(program)
}

// ---------------------------------------------------------------------------
// lockfile (wlel.lock)
// ---------------------------------------------------------------------------

/// `name = "revision"` lines; comments and blanks ignored
pub fn read_lock(dir: &Path) -> Vec<(String, String)> {
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
            if !k.is_empty() && !v.is_empty() {
                out.push((k.to_string(), v.to_string()));
            }
        }
    }
    out
}

pub fn write_lock(dir: &Path, entries: &[(String, String)]) {
    let mut body =
        String::from("# wlel.lock - resolved git dependency revisions (managed by wlel; commit this file)\n");
    for (k, v) in entries {
        body.push_str(&format!("{k} = \"{v}\"\n"));
    }
    let _ = fs::write(dir.join(LOCK_FILE), body);
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

/// clone `url` into `dir` (or reuse/repair an existing clone) and return the
/// resolved HEAD revision. A pinned `locked` revision is checked out when the
/// clone's HEAD does not match it — that is what makes builds reproducible.
fn fetch_git_dep(url: &str, dir: &Path, locked: Option<&str>) -> Result<String, String> {
    let checkout_locked = |dir: &Path, lc: &str| -> Result<(), String> {
        run_git(
            {
                let mut c = Command::new("git");
                c.arg("-C").arg(dir).arg("fetch").arg("--all");
                c
            },
            "fetch",
        )?;
        run_git(
            {
                let mut c = Command::new("git");
                c.arg("-C").arg(dir).arg("checkout").arg("--detach").arg(lc);
                c
            },
            &format!("checkout {lc}"),
        )?;
        Ok(())
    };
    if dir.join(".git").exists() {
        if let Some(lc) = locked {
            if git_head(dir)? != lc {
                checkout_locked(dir, lc)?;
            }
        }
    } else {
        if let Some(parent) = dir.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        run_git(
            {
                let mut c = Command::new("git");
                c.arg("clone").arg(url).arg(dir);
                c
            },
            &format!("clone {url}"),
        )?;
        if let Some(lc) = locked {
            if git_head(dir)? != lc {
                checkout_locked(dir, lc)?;
            }
        }
    }
    git_head(dir)
}

// ---------------------------------------------------------------------------
// scaffolding (wlel new)
// ---------------------------------------------------------------------------

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
    let toml = format!(
        "# {MANIFEST_FILE} - project manifest\n\
         [package]\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         \n\
         # Dependencies:\n\
         # [deps]\n\
         # mylib  = {{ path = \"../mylib\" }}\n\
         # json   = {{ git = \"https://github.com/user/wlel-json\" }}\n"
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
    let gitignore = format!("/{name}\n/{DEPS_DIR}/\n");
    let files: [(&str, String); 3] = [
        (MANIFEST_FILE, toml),
        ("src/main.wl", main_wl.into()),
        (".gitignore", gitignore),
    ];
    for (rel, content) in files {
        let p = dir.join(rel);
        fs::write(&p, content).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
    }
    Ok(dir.to_path_buf())
}
