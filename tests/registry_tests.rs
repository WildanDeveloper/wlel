//! Package registry v0: semver requirements, `wlel add`, git-tag resolution,
//! lockfile requirement pinning, offline behavior. Needs a C compiler and
//! git on PATH, like `wlel` itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use wlel::registry::{dep_name_from_url, parse_git_spec, Req, Version};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "wlel_reg_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        TempDir { path: dir }
    }

    /// run `wlel <args>` with `cwd` as the working directory
    fn wlel(&self, args: &[&str], cwd: &Path) -> (String, String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("spawn wlel");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }

    fn write(&self, rel: &str, content: &str) {
        let p = self.path.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("mkdir for file");
        }
        fs::write(p, content).expect("write file");
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path.join(rel)).expect("read file")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// ---------------------------------------------------------------------------
// semver + spec parsing (unit tests against the library)
// ---------------------------------------------------------------------------

#[test]
fn version_parses_and_orders() {
    assert_eq!(Version::parse("1.2.3").unwrap(), Version { major: 1, minor: 2, patch: 3 });
    assert_eq!(Version::parse("v1.2.3").unwrap(), Version { major: 1, minor: 2, patch: 3 });
    assert_eq!(Version::parse("1.2").unwrap(), Version { major: 1, minor: 2, patch: 0 });
    assert_eq!(Version::parse("2").unwrap(), Version { major: 2, minor: 0, patch: 0 });
    assert_eq!(Version::parse(" 0.1.0 ").unwrap(), Version { major: 0, minor: 1, patch: 0 });
    // ordering: 1.2.3 < 1.10.0 < 2.0.0
    let v = |s: &str| Version::parse(s).unwrap();
    assert!(v("1.2.3") < v("1.10.0") && v("1.10.0") < v("2.0.0"));
    for bad in ["", "1.x", "x.2.3", "1.2.3.4", "1..3", "-1.2.3", "1.2.x"] {
        assert!(Version::parse(bad).is_err(), "'{bad}' must not parse");
    }
}

#[test]
fn caret_requirements_match_like_cargo() {
    let r = |s: &str| Req::parse(s).unwrap();
    let v = |s: &str| Version::parse(s).unwrap();

    // ^1.2.3 -> >=1.2.3 <2.0.0
    let req = r("1.2.3");
    assert!(!req.matches(&v("1.2.2")));
    assert!(req.matches(&v("1.2.3")));
    assert!(req.matches(&v("1.9.9")));
    assert!(!req.matches(&v("2.0.0")));
    // the ^ prefix is accepted and equivalent
    assert!(r("^1.2.3").matches(&v("1.5.0")));

    // ^0.2.3 -> >=0.2.3 <0.3.0 (0.x never jumps minors)
    let req = r("0.2.3");
    assert!(!req.matches(&v("0.2.2")));
    assert!(req.matches(&v("0.2.9")));
    assert!(!req.matches(&v("0.3.0")));
    // ^0.0.3 -> >=0.0.3 <0.0.4
    let req = r("0.0.3");
    assert!(req.matches(&v("0.0.3")));
    assert!(!req.matches(&v("0.0.4")));
    // short forms: ^1 -> <2.0.0, ^0.2 -> <0.3.0, ^0.0 -> <0.1.0, ^0 -> <1.0.0
    assert!(r("1").matches(&v("1.99.0")) && !r("1").matches(&v("2.0.0")));
    assert!(r("0.2").matches(&v("0.2.7")) && !r("0.2").matches(&v("0.3.0")));
    assert!(r("0.0").matches(&v("0.0.9")) && !r("0.0").matches(&v("0.1.0")));
    assert!(r("0").matches(&v("0.9.9")) && !r("0").matches(&v("1.0.0")));

    for bad in ["", "x", "1.x", "latest"] {
        assert!(Req::parse(bad).is_err(), "'{bad}' must not parse");
    }
    // display round-trips as caret
    assert_eq!(format!("{}", r("1.2.3")), "^1.2.3");
}

#[test]
fn git_specs_expand_github_and_split_requirements() {
    let (url, req) = parse_git_spec("github:user/wlel-json").unwrap();
    assert_eq!(url, "https://github.com/user/wlel-json");
    assert_eq!(req, None);

    let (url, req) = parse_git_spec("github:user/wlel-json@1.2").unwrap();
    assert_eq!(url, "https://github.com/user/wlel-json");
    assert_eq!(req.as_deref(), Some("1.2"));

    // full URLs pass through, with or without a requirement
    let (url, req) = parse_git_spec("https://example.com/x/repo.git@1.0.0").unwrap();
    assert_eq!(url, "https://example.com/x/repo.git");
    assert_eq!(req.as_deref(), Some("1.0.0"));

    // scp-like git URLs keep their '@' (the tail is not a version)
    let (url, req) = parse_git_spec("git@github.com:user/repo").unwrap();
    assert_eq!(url, "git@github.com:user/repo");
    assert_eq!(req, None);

    // malformed shorthand is rejected
    assert!(parse_git_spec("github:noseparator").is_err());
    assert!(parse_git_spec("github:user/").is_err());

    // names derive from URLs (and local paths)
    assert_eq!(dep_name_from_url("https://github.com/user/wlel-json").as_deref(), Some("wlel-json"));
    assert_eq!(dep_name_from_url("https://github.com/user/repo.git").as_deref(), Some("repo"));
    assert_eq!(dep_name_from_url("/tmp/somewhere/pointlib").as_deref(), Some("pointlib"));
    assert_eq!(dep_name_from_url("https://host/"), None);
}

#[test]
fn manifest_accepts_version_requirements_for_git_deps_only() {
    let m = wlel::project::parse_manifest(
        "[package]\nname = \"app\"\n\n[deps]\njson = { git = \"https://x/y\", version = \"1.2\" }\n",
    )
    .unwrap();
    assert_eq!(m.deps.len(), 1);
    assert_eq!(m.deps[0].version.as_deref(), Some("1.2"));

    // path deps cannot carry a version
    let err = wlel::project::parse_manifest(
        "[deps]\nloc = { path = \"../loc\", version = \"1.0\" }\n",
    )
    .unwrap_err();
    assert!(err.contains("'version' applies to git dependencies only"), "{err}");

    // malformed requirements fail with the line number
    let err = wlel::project::parse_manifest(
        "[deps]\njson = { git = \"https://x/y\", version = \"latest\" }\n",
    )
    .unwrap_err();
    assert!(err.contains("line 2") && err.contains("invalid version requirement"), "{err}");
}

#[test]
fn lockfile_roundtrips_both_formats() {
    let t = TempDir::new("lockfmt");
    // legacy plain-revision format (pre-registry lockfiles) still reads
    t.write("wlel.lock", "# wlel.lock\nplain = \"abcdef1234567890\"\n");
    let entries = wlel::project::read_lock(&t.path);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "plain");
    assert_eq!(entries[0].req, None);
    assert_eq!(entries[0].rev, "abcdef1234567890");

    // requirement format: "req@rev"
    t.write("wlel.lock", "json = \"1.2@deadbeef\"\n");
    let entries = wlel::project::read_lock(&t.path);
    assert_eq!(entries[0].req.as_deref(), Some("1.2"));
    assert_eq!(entries[0].rev, "deadbeef");

    // writing reproduces the same encoding
    wlel::project::write_lock(
        &t.path,
        &[
            wlel::project::LockEntry {
                name: "json".into(),
                req: Some("1.2".into()),
                rev: "deadbeef".into(),
            },
            wlel::project::LockEntry { name: "plain".into(), req: None, rev: "abc123".into() },
        ],
    );
    let text = t.read("wlel.lock");
    assert!(text.contains("json = \"1.2@deadbeef\""), "{text}");
    assert!(text.contains("plain = \"abc123\""), "{text}");
}

// ---------------------------------------------------------------------------
// remote helper: a local git repo with tagged releases
// ---------------------------------------------------------------------------

/// build a local git "remote" whose tagged releases each carry their own
/// manifest version and library body: `(tag, version, lib source)`
fn init_remote_tagged(dir: &Path, name: &str, releases: &[(&str, &str, &str)]) {
    fs::create_dir_all(dir.join("src")).expect("mkdir remote src");
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let mut first = true;
    for (tag, ver, body) in releases {
        fs::write(
            dir.join("wlel.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"{ver}\"\n"),
        )
        .expect("write dep toml");
        fs::write(dir.join("src/lib.wl"), body).expect("write dep lib");
        if first {
            git(&["init", "-q", "--initial-branch=main"]);
            first = false;
        }
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
        git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", tag]);
        git(&["tag", tag]);
    }
}

fn remote_url(t: &TempDir, name: &str) -> String {
    // forward slashes: raw backslash temp paths would be TOML escapes
    t.path.join(name).display().to_string().replace('\\', "/")
}

// ---------------------------------------------------------------------------
// wlel add end-to-end
// ---------------------------------------------------------------------------

#[test]
fn add_resolves_the_best_tag_pins_the_lock_and_stays_offline() {
    let t = TempDir::new("addflow");
    init_remote_tagged(
        &t.path.join("remote"),
        "greetx",
        &[
            ("v1.0.0", "1.0.0", "fn hi() -> int { return 10; }"),
            ("v1.1.0", "1.1.0", "fn hi() -> int { return 11; }"),
            ("v2.0.0", "2.0.0", "fn hi() -> int { return 20; }"),
        ],
    );
    let url = remote_url(&t, "remote");

    t.wlel(&["new", "app"], &t.path);
    let app = t.path.join("app");
    t.write("app/src/main.wl", "use std;\nfn main() -> int {\n    std::println_int(hi());\n    return 0;\n}\n");

    // add with a requirement: the manifest gains the dep, the best matching
    // tag (v1.1.0 for ^1.0.0) is checked out and reported
    let (out, err, code) = t.wlel(&["add", "greetx", &format!("{url}@1.0.0")], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("added 'greetx'"), "{out}");
    assert!(out.contains("satisfies 1.0.0 via tag v1.1.0"), "{out}");

    let manifest = t.read("app/wlel.toml");
    assert!(
        manifest.contains(&format!("greetx = {{ git = \"{url}\", version = \"1.0.0\" }}")),
        "manifest must carry the requirement: {manifest}"
    );
    let lock = t.read("app/wlel.lock");
    assert!(lock.contains("greetx = \"1.0.0@"), "lock records requirement + revision: {lock}");

    // the resolved v1.1.0 body is what runs
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("11"), "{out}");

    // offline: with the remote gone, the pinned revision still builds and
    // produces no warning (the lock is reused without touching the network)
    fs::rename(t.path.join("remote"), t.path.join("remote-away")).expect("hide remote");
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("11"), "{out}");
    assert!(!err.contains("warning"), "no warnings expected offline: {err}");

    // raising the requirement re-resolves — offline, from the already-fetched
    // tags, with a warning that the refresh failed
    t.write(
        "app/wlel.toml",
        &format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[deps]\ngreetx = {{ git = \"{url}\", version = \"2.0.0\" }}\n"
        ),
    );
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("20"), "must resolve v2.0.0 from local tags: {out}");
    assert!(err.contains("offline"), "fallback must warn: {err}");
    let lock = t.read("app/wlel.lock");
    assert!(lock.contains("greetx = \"2.0.0@"), "{lock}");

    // remote back: plain add (no requirement) pins whatever HEAD is
    fs::rename(t.path.join("remote-away"), t.path.join("remote")).expect("restore remote");
    t.write("app/wlel.lock", "");
    t.wlel(&["add", "greetx", &url], &app); // duplicate — error path is tested elsewhere
}

#[test]
fn add_without_a_requirement_pins_head() {
    let t = TempDir::new("addhead");
    init_remote_tagged(
        &t.path.join("remote"),
        "loosex",
        &[("v0.1.0", "0.1.0", "fn hi() -> int { return 7; }")],
    );
    let url = remote_url(&t, "remote");

    t.wlel(&["new", "app"], &t.path);
    let app = t.path.join("app");
    t.write("app/src/main.wl", "use std;\nfn main() -> int {\n    std::println_int(hi());\n    return 0;\n}\n");

    let (out, err, code) = t.wlel(&["add", "loosex", &url], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("added 'loosex'"), "{out}");
    let manifest = t.read("app/wlel.toml");
    assert!(
        manifest.contains(&format!("loosex = {{ git = \"{url}\" }}")),
        "{manifest}"
    );
    let lock = t.read("app/wlel.lock");
    assert!(lock.contains("loosex = \""), "{lock}");
    assert!(!lock.contains("@"), "no requirement: plain revision: {lock}");

    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("7"), "{out}");
}

#[test]
fn add_errors_never_touch_the_manifest() {
    let t = TempDir::new("adderr");
    init_remote_tagged(
        &t.path.join("remote"),
        "greetx",
        &[
            ("v1.0.0", "1.0.0", "fn hi() -> int { return 10; }"),
            ("v1.1.0", "1.1.0", "fn hi() -> int { return 11; }"),
        ],
    );
    let url = remote_url(&t, "remote");

    t.wlel(&["new", "app"], &t.path);
    let app = t.path.join("app");
    let before = t.read("app/wlel.toml");

    // requirement nothing satisfies: lists the tags that exist
    let (_out, err, code) = t.wlel(&["add", "greetx", &format!("{url}@9.9.9")], &app);
    assert_ne!(code, Some(0));
    assert!(err.contains("no git tag satisfies requirement '^9.9.9'"), "{err}");
    assert!(err.contains("v1.0.0") && err.contains("v1.1.0"), "available tags listed: {err}");

    // tag says 1.x but the manifest inside says 0.1.0: semver check refuses
    init_remote_tagged(
        &t.path.join("badtag"),
        "badver",
        &[("v1.5.0", "0.1.0", "fn hi() -> int { return 1; }")],
    );
    let bad_url = remote_url(&t, "badtag");
    let (_out, err, code) = t.wlel(&["add", "badver", &format!("{bad_url}@1.0.0")], &app);
    assert_ne!(code, Some(0));
    assert!(
        err.contains("declares version 0.1.0 which does not satisfy requirement '1.0.0'"),
        "{err}"
    );

    // a dependency whose declared name does not match is rejected
    init_remote_tagged(
        &t.path.join("other"),
        "notgreetx",
        &[("v1.0.0", "1.0.0", "fn hi() -> int { return 1; }")],
    );
    let other_url = remote_url(&t, "other");
    let (_out, err, code) = t.wlel(&["add", "greetx", &other_url], &app);
    assert_ne!(code, Some(0));
    assert!(err.contains("declares name 'notgreetx'"), "{err}");

    // a repo without src/lib.wl is rejected (built without one from the start)
    let nolib_dir = t.path.join("nolib");
    fs::create_dir_all(&nolib_dir).expect("mkdir nolib");
    fs::write(
        nolib_dir.join("wlel.toml"),
        "[package]\nname = \"nolib\"\nversion = \"1.0.0\"\n",
    )
    .expect("write toml");
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&nolib_dir)
            .output()
            .expect("run git");
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "-q", "--initial-branch=main"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "v1"]);
    git(&["tag", "v1.0.0"]);
    let nolib_url = remote_url(&t, "nolib");
    let (_out, err, code) = t.wlel(&["add", "nolib", &nolib_url], &app);
    assert_ne!(code, Some(0));
    assert!(err.contains("no src/lib.wl"), "{err}");

    // the manifest is byte-identical and no lock was written
    assert_eq!(t.read("app/wlel.toml"), before, "failed adds must not edit the manifest");
    assert!(!app.join("wlel.lock").exists(), "failed adds must not write the lock");

    // a successful add makes a second identical add an error
    let (out, err, code) = t.wlel(&["add", "greetx", &format!("{url}@1.0.0")], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    let (_out, err, code) = t.wlel(&["add", "greetx", &format!("{url}@1.0.0")], &app);
    assert_ne!(code, Some(0));
    assert!(err.contains("already declared"), "{err}");
}

#[test]
fn add_path_dep_creates_the_deps_section_and_derives_the_name() {
    let t = TempDir::new("addpath");
    t.write(
        "pointlib/wlel.toml",
        "[package]\nname = \"pointlib\"\nversion = \"0.1.0\"\n",
    );
    t.write("pointlib/src/lib.wl", "fn triple(x: int) -> int { return x * 3; }\n");

    t.wlel(&["new", "app"], &t.path);
    let app = t.path.join("app");
    t.write("app/src/main.wl", "use std;\nfn main() -> int {\n    std::println_int(triple(14));\n    return 0;\n}\n");

    // no [deps] section yet: `wlel add --path` creates it, name derives from
    // the directory
    let (out, err, code) = t.wlel(&["add", "--path", "../pointlib"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("added 'pointlib' (path = \"../pointlib\")"), "{out}");

    let manifest = t.read("app/wlel.toml");
    assert!(manifest.contains("[deps]\npointlib = { path = \"../pointlib\" }"), "{manifest}");

    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("42"), "{out}");

    // an explicit name is required to differ from the directory... and a
    // mismatch with the dep's own manifest is an error
    let (_out, err, code) = t.wlel(&["add", "wrongname", "--path", "../pointlib"], &app);
    assert_ne!(code, Some(0));
    assert!(err.contains("declares name 'pointlib'"), "{err}");
}

#[test]
fn add_outside_a_project_is_a_clear_error() {
    let t = TempDir::new("addnoproj");
    let (_out, err, code) = t.wlel(&["add", "github:user/repo"], &t.path);
    assert_ne!(code, Some(0));
    assert!(err.contains("no wlel.toml"), "{err}");
}
