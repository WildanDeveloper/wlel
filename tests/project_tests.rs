//! Project mode end-to-end tests: `wlel new`, `wlel.toml` manifests,
//! multi-file projects, path dependencies, git dependencies + `wlel.lock`.
//! Needs a C compiler and git on PATH, like `wlel` itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "wlel_proj_{}_{}",
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
// manifest parser (unit tests against the library)
// ---------------------------------------------------------------------------

#[test]
fn manifest_parses_package_and_both_dep_kinds() {
    let m = wlel::project::parse_manifest(
        r#"
# comment
[package]
name = "myapp"
version = "1.2.3"

[deps]
pointlib = { path = "../pointlib" }   # trailing comment
json     = { git = "https://github.com/user/wlel-json" }
"#,
    )
    .expect("parse manifest");
    assert_eq!(m.name, "myapp");
    assert_eq!(m.version, "1.2.3");
    assert_eq!(m.deps.len(), 2);
    assert!(matches!(&m.deps[0].source, wlel::project::DepSource::Path(p) if p == "../pointlib"));
    assert!(matches!(&m.deps[1].source, wlel::project::DepSource::Git(u) if u == "https://github.com/user/wlel-json"));
}

#[test]
fn manifest_defaults_and_errors() {
    // version is optional
    let m = wlel::project::parse_manifest("[package]\nname = \"x\"\n").unwrap();
    assert_eq!(m.version, "0.1.0");
    assert!(m.deps.is_empty());

    // missing name
    assert!(wlel::project::parse_manifest("[package]\nversion = \"1.0\"\n")
        .unwrap_err()
        .contains("name is required"));
    // unknown section
    assert!(wlel::project::parse_manifest("[pakage]\nname = \"x\"\n")
        .unwrap_err()
        .contains("unknown section"));
    // unknown key
    assert!(wlel::project::parse_manifest("[package]\nname = \"x\"\nnaem = \"y\"\n")
        .unwrap_err()
        .contains("unknown key 'naem'"));
    // unquoted value
    assert!(wlel::project::parse_manifest("[package]\nname = x\n")
        .unwrap_err()
        .contains("expected a quoted string"));
    // both path and git
    assert!(wlel::project::parse_manifest(
        "[deps]\nx = { path = \"a\", git = \"b\" }\n"
    )
    .unwrap_err()
    .contains("both 'path' and 'git'"));
    // neither path nor git
    assert!(
        wlel::project::parse_manifest("[deps]\nx = { }\n")
            .unwrap_err()
            .contains("needs 'path' or 'git'")
    );
    assert!(
        wlel::project::parse_manifest("[deps]\nx = { url = \"b\" }\n")
            .unwrap_err()
            .contains("unknown key 'url'")
    );
    // duplicate dependency
    assert!(wlel::project::parse_manifest(
        "[deps]\nx = { path = \"a\" }\nx = { path = \"b\" }\n"
    )
    .unwrap_err()
    .contains("duplicate dependency"));
    // path traversal in the name (it becomes a directory under .wlel/deps)
    assert!(wlel::project::parse_manifest("[deps]\n../evil = { path = \"a\" }\n")
        .unwrap_err()
        .contains("invalid dependency name"));
    // key before any section
    assert!(wlel::project::parse_manifest("name = \"x\"\n")
        .unwrap_err()
        .contains("expected a [package]"));
}

// ---------------------------------------------------------------------------
// wlel new + project run/test/check/build
// ---------------------------------------------------------------------------

#[test]
fn new_scaffolds_a_runnable_project() {
    let t = TempDir::new("scaffold");
    let (out, err, code) = t.wlel(&["new", "app"], &t.path);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(t.path.join("app/wlel.toml").is_file());
    assert!(t.path.join("app/src/main.wl").is_file());

    let app = t.path.join("app");
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("hello, wlel!"), "{out}");

    // scaffolded test block runs via `wlel test`
    let (out, err, code) = t.wlel(&["test"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("pass: sanity"), "{out}");
}

#[test]
fn new_refuses_nonempty_directory() {
    let t = TempDir::new("refuse");
    // the target directory itself already contains something
    t.write("app/keep.txt", "x");
    let (_out, err, code) = t.wlel(&["new", "app"], &t.path);
    assert_ne!(code, Some(0));
    assert!(err.contains("not empty"), "{err}");
}

#[test]
fn project_multifile_and_project_test() {
    let t = TempDir::new("multifile");
    t.wlel(&["new", "app"], &t.path);
    t.write(
        "app/wlel.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    );
    t.write(
        "app/src/main.wl",
        r#"use "util.wl";
use std;

fn main() -> int {
    std::println_int(twice(21));
    return 0;
}"#,
    );
    t.write(
        "app/src/util.wl",
        r#"fn twice(x: int) -> int { return x * 2; }
test "menggandakan" {
    assert_eq(twice(21), 42);
}"#,
    );
    let app = t.path.join("app");
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("42"), "{out}");
    // tests from imported files are part of the project suite
    let (out, err, code) = t.wlel(&["test"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("pass: menggandakan"), "{out}");
}

#[test]
fn run_forwards_args_to_the_program() {
    let t = TempDir::new("runargs");
    t.wlel(&["new", "app"], &t.path);
    t.write(
        "app/src/main.wl",
        r#"use std;
fn main() -> int {
    if sys::argc() > 1 {
        std::println_str(sys::arg(1));
    }
    return 0;
}"#,
    );
    let app = t.path.join("app");
    let (out, err, code) = t.wlel(&["run", "--", "halo"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("halo"), "{out}");
}

#[test]
fn project_build_names_binary_after_package_and_emit_c_works() {
    let t = TempDir::new("buildname");
    t.wlel(&["new", "aplikasi"], &t.path);
    let app = t.path.join("aplikasi");
    let (_out, err, code) = t.wlel(&["build"], &app);
    assert_eq!(code, Some(0), "{err}");
    assert!(app.join("aplikasi").is_file(), "binary named after the package");

    let (_out, err, code) = t.wlel(&["build", "--emit-c", "-o", "custom"], &app);
    assert_eq!(code, Some(0), "{err}");
    assert!(app.join("custom").is_file());
    assert!(app.join("custom.c").is_file());
}

// ---------------------------------------------------------------------------
// path dependencies
// ---------------------------------------------------------------------------

#[test]
fn path_dep_merges_functions_structs_and_tests() {
    let t = TempDir::new("pathdep");
    t.write(
        "pointlib/wlel.toml",
        "[package]\nname = \"pointlib\"\nversion = \"0.1.0\"\n",
    );
    t.write(
        "pointlib/src/lib.wl",
        r#"use std;

struct Pt {
    x: int,
    y: int,
}

fn mk(x: int, y: int) -> Pt {
    return Pt { x: x, y: y };
}

fn sum(p: *Pt) -> int {
    v := vec_new[int]();
    vec_push(&v, p.x);
    vec_push(&v, p.y);
    return vec_get(&v, 0) + vec_get(&v, 1);
}

test "pt sum" {
    p := mk(40, 2);
    assert_eq(sum(&p), 42);
}"#,
    );
    t.wlel(&["new", "app"], &t.path);
    t.write(
        "app/wlel.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[deps]\npointlib = { path = \"../pointlib\" }\n",
    );
    // main deliberately has NO `use std`: the dependency's `use std` must
    // pull in the embedded library for the whole merge
    t.write(
        "app/src/main.wl",
        r#"fn main() -> int {
    p := mk(40, 2);
    return sum(&p);
}"#,
    );
    let app = t.path.join("app");
    // main returns sum(&p) — the process exit code IS the computed sum: 42
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(42), "stdout: {out}\nstderr: {err}");
    t.write(
        "app/src/main.wl",
        r#"use std;
fn main() -> int {
    p := mk(40, 2);
    std::println_int(sum(&p));
    return 0;
}"#,
    );
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("42"), "{out}");
    // the dependency's test block runs in the project suite
    let (out, err, code) = t.wlel(&["test"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("pass: pt sum"), "{out}");
}

#[test]
fn path_dep_errors_are_actionable() {
    let t = TempDir::new("pathdeperr");
    // dep is not a wlel project (missing wlel.toml)
    t.write("bare/lib.wl", "fn x() -> int { return 1; }");
    t.wlel(&["new", "app"], &t.path);
    t.write(
        "app/wlel.toml",
        "[package]\nname = \"app\"\n\n[deps]\nbare = { path = \"../bare\" }\n",
    );
    t.write("app/src/main.wl", "fn main() -> int { return 0; }");
    let (_out, err, code) = t.wlel(&["check"], &t.path.join("app"));
    assert_ne!(code, Some(0));
    assert!(err.contains("not a wlel project"), "{err}");

    // dep name mismatch
    t.write(
        "mismatch/wlel.toml",
        "[package]\nname = \"other\"\n",
    );
    t.write("mismatch/src/lib.wl", "fn x() -> int { return 1; }");
    t.write(
        "app/wlel.toml",
        "[package]\nname = \"app\"\n\n[deps]\nmismatch = { path = \"../mismatch\" }\n",
    );
    let (_out, err, code) = t.wlel(&["check"], &t.path.join("app"));
    assert_ne!(code, Some(0));
    assert!(err.contains("declares name 'other'"), "{err}");

    // dep without src/lib.wl
    t.write("nolib/wlel.toml", "[package]\nname = \"nolib\"\n");
    t.write(
        "app/wlel.toml",
        "[package]\nname = \"app\"\n\n[deps]\nnolib = { path = \"../nolib\" }\n",
    );
    let (_out, err, code) = t.wlel(&["check"], &t.path.join("app"));
    assert_ne!(code, Some(0));
    assert!(err.contains("no src/lib.wl"), "{err}");
}

// ---------------------------------------------------------------------------
// git dependencies + lockfile
// ---------------------------------------------------------------------------

/// create a local git repo usable as a dependency "remote"
fn init_remote(dir: &Path, name: &str, body: &str) -> String {
    fs::create_dir_all(dir.join("src")).expect("mkdir remote src");
    fs::write(
        dir.join("wlel.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n"),
    )
    .expect("write dep toml");
    fs::write(dir.join("src/lib.wl"), body).expect("write dep lib");
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
    git(&["init", "-q", "--initial-branch=main"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"]);
    git(&["rev-parse", "HEAD"])
}

#[test]
fn git_dep_clones_builds_and_lock_pins_the_revision() {
    let t = TempDir::new("gitdep");
    let sha1 = init_remote(
        &t.path.join("remote"),
        "greetx",
        "fn hi() -> int { return 41; }",
    );

    t.wlel(&["new", "app"], &t.path);
    t.write(
        "app/wlel.toml",
        &format!(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[deps]\ngreetx = {{ git = \"{}\" }}\n",
            t.path.join("remote").display()
        ),
    );
    t.write(
        "app/src/main.wl",
        r#"use std;
fn main() -> int {
    std::println_int(hi());
    return 0;
}"#,
    );
    let app = t.path.join("app");

    // first build: clones the remote and writes the lockfile
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("41"), "{out}");
    assert!(app.join(".wlel/deps/greetx/.git").exists(), "dep cloned");
    let lock = t.read("app/wlel.lock");
    assert!(lock.contains(&sha1), "lock pins the resolved revision: {lock}");

    // the remote moves on, but the lock keeps builds reproducible even
    // after the clone cache is wiped
    fs::write(
        t.path.join("remote/src/lib.wl"),
        "fn hi() -> int { return 42; }",
    )
    .expect("advance remote");
    let git = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qam", "bump"])
        .current_dir(t.path.join("remote"))
        .output()
        .expect("commit");
    assert!(git.status.success(), "{}", String::from_utf8_lossy(&git.stderr));

    let _ = fs::remove_dir_all(app.join(".wlel"));
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("41"), "locked revision must still build: {out}");

    // deleting the lock unlocks: HEAD of the remote is resolved fresh
    let _ = fs::remove_file(app.join("wlel.lock"));
    let _ = fs::remove_dir_all(app.join(".wlel"));
    let (out, err, code) = t.wlel(&["run"], &app);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("42"), "unlocked build tracks the remote: {out}");
}

#[test]
fn library_project_checks_and_tests_but_cannot_build() {
    let t = TempDir::new("libproj");
    t.write(
        "wlel.toml",
        "[package]\nname = \"mathx\"\nversion = \"0.1.0\"\n",
    );
    t.write(
        "src/lib.wl",
        r#"fn square(x: int) -> int { return x * x; }
test "square" {
    assert_eq(square(6), 36);
}"#,
    );
    let (_out, err, code) = t.wlel(&["check"], &t.path);
    assert_eq!(code, Some(0), "{err}");
    let (out, err, code) = t.wlel(&["test"], &t.path);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("pass: square"), "{out}");
    let (_out, err, code) = t.wlel(&["build"], &t.path);
    assert_ne!(code, Some(0));
    assert!(err.contains("library"), "{err}");
    let (_out, err, code) = t.wlel(&["run"], &t.path);
    assert_ne!(code, Some(0));
    assert!(err.contains("library"), "{err}");
}

#[test]
fn running_outside_a_project_is_a_clear_error() {
    let t = TempDir::new("noproject");
    let (_out, err, code) = t.wlel(&["run"], &t.path);
    assert_ne!(code, Some(0));
    assert!(err.contains("no wlel.toml"), "{err}");
    let (_out, _err, code) = t.wlel(&["build"], &t.path);
    assert_ne!(code, Some(0));
}

#[test]
fn project_finds_wlel_toml_in_parent_directories() {
    let t = TempDir::new("walkup");
    t.wlel(&["new", "app"], &t.path);
    let app = t.path.join("app");
    // run from a nested subdirectory of the project
    fs::create_dir_all(app.join("a/b/c")).expect("mkdir nested");
    let (out, err, code) = t.wlel(&["run"], &app.join("a/b/c"));
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    assert!(out.contains("hello, wlel!"), "{out}");
}
