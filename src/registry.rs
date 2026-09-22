//! Package registry v0: version requirements for git dependencies and the
//! `wlel add` command — no central server, git tags are the registry.
//!
//! A dependency may pin a version requirement in the manifest:
//!
//! ```toml
//! [deps]
//! json = { git = "https://github.com/user/wlel-json", version = "1.2" }
//! ```
//!
//! The requirement is caret semantics (like Cargo's `^`): `1.2` matches
//! `>=1.2.0 <2.0.0`, `0.2.3` matches `>=0.2.3 <0.3.0` (0.x releases never
//! jump minors), `0.0.3` matches `>=0.0.3 <0.0.4`. Resolution picks the
//! highest `git tag` satisfying the requirement (tags `v1.2.3` and `1.2.3`
//! are both accepted), checks out that tag, and then verifies the tag's own
//! `wlel.toml` declares a compatible version — the tag and the manifest must
//! tell the same story. `wlel.lock` records the requirement together with
//! the resolved revision, so builds stay reproducible and a changed
//! requirement re-resolves automatically.
//!
//! Everything prefers the local clone: once a revision is locked, later
//! builds never touch the network, and even re-resolution falls back to the
//! tags already fetched when the remote is unreachable (offline-friendly).

use crate::project;
use std::fs;
use std::path::Path;

// ---------------------------------------------------------------------------
// semver (major.minor.patch) with Cargo-style caret requirements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// `1.2.3`, `v1.2.3`, `1.2`, `1` — missing components are zero
    pub fn parse(s: &str) -> Result<Version, String> {
        let t = s.trim();
        let t = t.strip_prefix('v').unwrap_or(t);
        if t.is_empty() {
            return Err("empty version".into());
        }
        let mut nums = [0u64; 3];
        for (i, part) in t.split('.').enumerate() {
            if i >= 3 {
                return Err(format!("invalid version '{s}' (too many components)"));
            }
            if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!(
                    "invalid version '{s}' (expected major[.minor][.patch] digits)"
                ));
            }
            nums[i] = part.parse().map_err(|_| format!("invalid version '{s}'"))?;
        }
        Ok(Version { major: nums[0], minor: nums[1], patch: nums[2] })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// a caret requirement `^X.Y.Z` (the `^` itself is optional in manifests)
#[derive(Debug, Clone)]
pub struct Req {
    min: Version,
    /// how many components the author wrote (`1` vs `1.2` vs `1.2.3`) —
    /// distinguishes `^0.0` (`<0.1.0`) from `^0.0.0` (`<0.0.1`)
    written: usize,
}

impl Req {
    pub fn parse(s: &str) -> Result<Req, String> {
        let t = s.trim();
        if t.is_empty() {
            return Err("empty version requirement".into());
        }
        let t = t.strip_prefix('^').unwrap_or(t);
        let min = Version::parse(t)?;
        let written = t.split('.').count();
        Ok(Req { min, written })
    }

    pub fn matches(&self, v: &Version) -> bool {
        if *v < self.min {
            return false;
        }
        let upper = match (self.min.major, self.min.minor, self.written) {
            (m, _, w) if m > 0 || w == 1 => Version { major: self.min.major + 1, minor: 0, patch: 0 },
            (0, n, w) if n > 0 || w == 2 => Version { major: 0, minor: self.min.minor + 1, patch: 0 },
            _ => Version { major: 0, minor: 0, patch: self.min.patch + 1 },
        };
        *v < upper
    }
}

impl std::fmt::Display for Req {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "^{}", self.min)
    }
}

// ---------------------------------------------------------------------------
// dependency specs on the command line
// ---------------------------------------------------------------------------

/// `github:owner/repo[@req]` → `https://github.com/owner/repo`; anything
/// else is a git URL (or local path) used verbatim. A trailing `@req` is
/// only treated as a version requirement when it parses as one, so scp-like
/// URLs (`git@github.com:user/repo`) survive.
pub fn parse_git_spec(spec: &str) -> Result<(String, Option<String>), String> {
    let (url, req) = match spec.rfind('@') {
        Some(i) if i > 0 && Req::parse(&spec[i + 1..]).is_ok() => {
            (spec[..i].to_string(), Some(spec[i + 1..].to_string()))
        }
        _ => (spec.to_string(), None),
    };
    let url = match url.strip_prefix("github:") {
        Some(rest) => {
            let rest = rest.strip_suffix(".git").unwrap_or(rest);
            let mut it = rest.split('/');
            let ok = matches!((it.next(), it.next(), it.next()),
                (Some(o), Some(r), None) if !o.is_empty() && !r.is_empty());
            if !ok {
                return Err(format!(
                    "invalid github spec '{spec}' (expected github:owner/repo)"
                ));
            }
            format!("https://github.com/{rest}")
        }
        None => url,
    };
    Ok((url, req))
}

/// last path component of a git URL (`.git` suffix stripped) — the default
/// dependency name; for scheme URLs the host never counts as the name
pub fn dep_name_from_url(url: &str) -> Option<String> {
    let last = |s: &str| -> Option<String> {
        let s = s.trim_end_matches('/');
        let s = s.strip_suffix(".git").unwrap_or(s);
        let last = s.rsplit(['/', '\\', ':']).next().unwrap_or("");
        if last.is_empty() {
            None
        } else {
            Some(last.to_string())
        }
    };
    match url.trim().split_once("://") {
        // https://host/user/repo -> look only at the path after the host
        Some((_, rest)) => {
            let path = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
            last(path)
        }
        None => last(url.trim()),
    }
}

// ---------------------------------------------------------------------------
// wlel add
// ---------------------------------------------------------------------------

pub enum AddSource {
    /// a git spec (`github:owner/repo`, URL, or local path), `@req` allowed
    Git(String),
    /// a path dependency directory, resolved relative to the project root
    Path(String),
}

/// `wlel add [<name>] <spec> | --path <dir>`: validate, resolve, edit
/// `wlel.toml`, then resolve the whole project (which writes `wlel.lock`).
/// The manifest is restored untouched when anything fails.
pub fn add_dependency(
    dir: &Path,
    name: Option<String>,
    source: AddSource,
) -> Result<(String, Vec<String>), String> {
    let manifest_path = dir.join(project::MANIFEST_FILE);
    let manifest_src = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    let manifest = project::parse_manifest(&manifest_src)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;

    let mut warnings = Vec::new();
    let (name, source_desc, table, resolved_note) = match source {
        AddSource::Git(spec) => {
            let (url, req) = parse_git_spec(&spec)?;
            let name = match name {
                Some(n) => n,
                None => dep_name_from_url(&url)
                    .ok_or_else(|| format!("cannot derive a dependency name from '{url}' (pass one: wlel add <name> {spec})"))?,
            };
            if let Some(r) = &req {
                Req::parse(r).map_err(|e| format!("invalid version requirement '{r}': {e}"))?;
            }
            // resolve now (clones if needed, picks the tag, verifies the
            // dependency's own manifest) so a broken add never edits wlel.toml
            let cache = project::git_dep_cache_dir(dir, &name);
            let gr = project::fetch_git_dep(&url, &cache, req.as_deref(), None, &mut warnings)?;
            check_dep_shape(&cache, &name, req.as_deref())?;
            let table = match &req {
                Some(r) => format!("{{ git = \"{url}\", version = \"{r}\" }}"),
                None => format!("{{ git = \"{url}\" }}"),
            };
            let note = match (&gr.tag, &req) {
                (Some(t), Some(r)) => format!(" (satisfies {r} via tag {t})"),
                (Some(t), None) => format!(" (tag {t})"),
                (None, Some(r)) => format!(" (satisfies {r})"),
                (None, None) => String::new(),
            };
            let desc = format!("git = \"{url}\"");
            (name, desc, table, note)
        }
        AddSource::Path(p) => {
            let name = match name {
                Some(n) => n,
                None => dep_name_from_url(&p)
                    .ok_or_else(|| format!("cannot derive a dependency name from '{p}' (pass one: wlel add <name> --path {p})"))?,
            };
            // same rule as the manifest: relative to the project root; TOML
            // strings cannot carry raw backslashes
            let p = p.replace('\\', "/");
            let dep_root = dir.join(&p);
            check_dep_shape(&dep_root, &name, None)?;
            let table = format!("{{ path = \"{p}\" }}");
            let desc = format!("path = \"{p}\"");
            (name, desc, table, String::new())
        }
    };

    if !project::valid_dep_name(&name) {
        return Err(format!(
            "invalid dependency name '{name}' (use letters, digits, '-', '_', '.')"
        ));
    }
    if manifest.deps.iter().any(|d| d.name == name) {
        return Err(format!(
            "dependency '{name}' is already declared in {} (edit the manifest to change it)",
            manifest_path.display()
        ));
    }
    let dep_line = format!("{name} = {table}");

    let new_src = insert_dep_line(&manifest_src, &dep_line);
    // guard against our own writer bugs: the edited manifest must still parse
    // and carry the new dependency before it touches the disk
    let edited = project::parse_manifest(&new_src)
        .map_err(|e| format!("internal error: edited manifest no longer parses: {e}"))?;
    if !edited.deps.iter().any(|d| d.name == name) {
        return Err("internal error: edited manifest lost the new dependency".into());
    }
    fs::write(&manifest_path, &new_src)
        .map_err(|e| format!("cannot write {}: {e}", manifest_path.display()))?;

    // full resolve: merges every dep, rewrites wlel.lock — on failure the
    // manifest goes back exactly as it was
    if let Err(e) = project::load_project(dir) {
        let _ = fs::write(&manifest_path, &manifest_src);
        return Err(e);
    }
    let pin = if resolved_note.is_empty() { String::new() } else { resolved_note };
    Ok((format!("added '{name}' ({source_desc}){pin}"), warnings))
}

/// the dependency must be a well-formed wlel library project whose declared
/// name matches, and (when a requirement is given) whose manifest version
/// satisfies it — the same checks `load_project` applies, but before the
/// manifest is touched
fn check_dep_shape(dep_root: &Path, name: &str, req: Option<&str>) -> Result<(), String> {
    let mf_path = dep_root.join(project::MANIFEST_FILE);
    let src = fs::read_to_string(&mf_path)
        .map_err(|e| format!("dependency '{name}': cannot read {}: {e}", mf_path.display()))?;
    let mf = project::parse_manifest(&src)
        .map_err(|e| format!("{}: {e}", mf_path.display()))?;
    if mf.name != name {
        return Err(format!(
            "dependency '{name}' declares name '{}' in its wlel.toml",
            mf.name
        ));
    }
    if !dep_root.join("src/lib.wl").is_file() {
        return Err(format!(
            "dependency '{name}' ({}) has no src/lib.wl",
            dep_root.display()
        ));
    }
    if let Some(r) = req {
        let req = Req::parse(r).map_err(|e| format!("invalid requirement '{r}': {e}"))?;
        let v = Version::parse(&mf.version)
            .map_err(|e| format!("dependency '{name}': {e}"))?;
        if !req.matches(&v) {
            return Err(format!(
                "dependency '{name}' version {} does not satisfy requirement '{r}'",
                mf.version
            ));
        }
    }
    Ok(())
}

/// insert `dep_line` at the end of the `[deps]` section (creating the
/// section at the end of the file when absent); everything else is kept
/// byte-for-byte
fn insert_dep_line(src: &str, dep_line: &str) -> String {
    let raw: Vec<&str> = src.lines().collect();
    let Some(h) = raw.iter().position(|l| l.trim() == "[deps]") else {
        let mut out = src.trim_end().to_string();
        out.push_str("\n\n[deps]\n");
        out.push_str(dep_line);
        out.push('\n');
        return out;
    };
    let end = raw[h + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map(|i| h + 1 + i)
        .unwrap_or(raw.len());
    let mut at = h;
    for (i, l) in raw.iter().enumerate().take(end).skip(h + 1) {
        if !l.trim().is_empty() {
            at = i;
        }
    }
    let mut out: Vec<&str> = Vec::with_capacity(raw.len() + 1);
    for (i, l) in raw.iter().enumerate() {
        out.push(l);
        if i == at {
            out.push(dep_line);
        }
    }
    let mut s = out.join("\n");
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}
