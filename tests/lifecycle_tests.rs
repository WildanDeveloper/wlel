//! Dependency lifecycle: `wlel init`, `remove`, `update`, `tree`, `clean`.
//! These edit `wlel.toml` / `wlel.lock` and the `.wlel/deps` clone cache, so
//! the tests drive the real binary and check the files byte-for-byte.
//! Needs a C compiler and git on PATH, like `wlel` itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Case {
    dir: PathBuf,
}

impl Case {
    fn new(name: &str) -> Case {
        let dir = std::env::temp_dir().join(format!(
            "wlel_life_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        Case { dir }
    }

    fn wlel(&self, args: &[&str]) -> (String, String, Option<i32>) {
        self.wlel_in(&self.dir, args)
    }

    fn wlel_in(&self, cwd: &Path, args: &[&str]) -> (String, String, Option<i32>) {
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
        let p = self.dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(p, content).expect("write");
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.dir.join(rel)).expect("read")
    }

    fn exists(&self, rel: &str) -> bool {
        self.dir.join(rel).exists()
    }

    fn ok(&self, args: &[&str]) -> String {
        let (out, err, code) = self.wlel(args);
        assert_eq!(code, Some(0), "wlel {args:?} failed: {out}{err}");
        out
    }

    /// run `wlel` inside `sub` (a directory under the case root)
    fn ok_in(&self, sub: &Path, args: &[&str]) -> String {
        let (out, err, code) = self.wlel_in(sub, args);
        assert_eq!(code, Some(0), "wlel {args:?} in {} failed: {out}{err}", sub.display());
        out
    }

    fn fail(&self, args: &[&str]) -> String {
        let (out, err, code) = self.wlel(args);
        assert_eq!(code, Some(1), "wlel {args:?} should fail: {out}{err}");
        err
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &Path, args: &[&str]) {
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
}

/// a library project at `dir` with the given name and body
fn lib_project(dir: &Path, name: &str, version: &str, body: &str) {
    fs::create_dir_all(dir.join("src")).expect("mkdir lib");
    fs::write(
        dir.join("wlel.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n"),
    )
    .expect("write toml");
    fs::write(dir.join("src/lib.wlel"), body).expect("write lib");
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

#[test]
fn init_adopts_an_existing_directory_without_overwriting() {
    let c = Case::new("init_existing");
    c.write("src/main.wlel", "fn main() -> int { return 7; }\n");
    c.write("README.md", "# my readme\n");

    let out = c.ok(&["init"]);
    assert!(out.contains("initialized"), "{out}");

    // the manifest and the project metadata appear
    assert!(c.exists("wlel.toml"), "manifest missing");
    assert!(c.exists(".gitignore"), "gitignore missing");
    assert!(c.exists(".github/workflows/ci.yml"), "CI template missing");
    // ... but existing work is untouched
    assert_eq!(c.read("src/main.wlel"), "fn main() -> int { return 7; }\n");
    assert_eq!(c.read("README.md"), "# my readme\n");

    // the scaffolded project is valid; the files init wrote are canonical
    // (the pre-existing main.wlel is the test's own, deliberately unformatted)
    let out = c.ok(&["check"]);
    assert!(out.contains("type-checks"), "{out}");
    assert!(c.exists("wlel.toml"), "manifest missing");
}

#[test]
fn init_creates_an_entry_point_when_there_is_none() {
    let c = Case::new("init_empty");
    c.ok(&["init"]);
    assert!(c.exists("src/main.wlel"), "no entry point written");
    let out = c.ok(&["run"]);
    assert!(out.contains("hello, wlel!"), "{out}");
}

#[test]
fn init_refuses_a_directory_that_already_is_a_project() {
    let c = Case::new("init_twice");
    c.ok(&["init"]);
    let err = c.fail(&["init"]);
    assert!(err.contains("already has a wlel.toml"), "{err}");
}

#[test]
fn the_scaffolded_ci_template_installs_the_matching_release() {
    let c = Case::new("init_ci");
    c.ok(&["new", c.dir.join("app").to_str().unwrap()]);
    let ci = fs::read_to_string(c.dir.join("app/.github/workflows/ci.yml")).expect("read ci");
    let version = env!("CARGO_PKG_VERSION");
    // the template must pin the same release the compiler reports, or a new
    // project would build against a compiler nobody tested
    assert!(
        ci.contains(&format!("/download/v{version}/wlel-{version}-")),
        "CI template does not pin v{version}: {ci}"
    );
    assert!(ci.contains("wlel check"), "{ci}");
    assert!(ci.contains("wlel test"), "{ci}");
    assert!(ci.contains("wlel fmt -check"), "{ci}");
    // the release asset name in the template must be one we actually publish
    assert!(
        ci.contains("x86_64-unknown-linux-gnu.tar.gz"),
        "unexpected asset name: {ci}"
    );
}

// ---------------------------------------------------------------------------
// tree
// ---------------------------------------------------------------------------

#[test]
fn tree_shows_path_and_git_dependencies_with_their_versions() {
    let c = Case::new("tree");
    lib_project(&c.dir.join("util"), "util", "0.2.0", "fn twice(x: int) -> int { return x * 2; }\n");
    let remote = c.dir.join("remote");
    lib_project(&remote, "greetx", "1.0.0", "fn hi() -> int { return 10; }\n");
    git(&remote, &["init", "-q", "--initial-branch=main"]);
    git(&remote, &["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
    git(&remote, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "v1"]);
    git(&remote, &["tag", "v1.0.0"]);

    let app = c.dir.join("app");
    c.ok(&["new", app.to_str().unwrap()]);
    c.ok_in(&app, &["add", "--path", "../util"]);
    let remote_s = "../remote".to_string();
    let (out, err, code) = c.wlel_in(&app, &["add", "greetx", &remote_s]);
    assert_eq!(code, Some(0), "add failed: {out}{err}");

    let tree = c.ok_in(&app, &["tree"]);
    assert!(tree.contains("app v0.1.0"), "{tree}");
    assert!(tree.contains("util v0.2.0 (path ../util)"), "{tree}");
    assert!(tree.contains("greetx v1.0.0 (git ../remote @ "), "{tree}");
}

#[test]
fn tree_reports_an_unresolved_dependency_instead_of_failing() {
    let c = Case::new("tree_unresolved");
    c.write(
        "wlel.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[deps]\nghost = { git = \"../nowhere\" }\n",
    );
    c.write("src/main.wlel", "fn main() -> int { return 0; }\n");
    let tree = c.ok(&["tree"]);
    assert!(tree.contains("ghost"), "{tree}");
    assert!(tree.contains("not resolved"), "{tree}");
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

#[test]
fn remove_drops_the_manifest_line_the_pin_and_the_clone() {
    let c = Case::new("remove");
    lib_project(&c.dir.join("util"), "util", "0.1.0", "fn twice(x: int) -> int { return x * 2; }\n");
    let remote = c.dir.join("remote");
    lib_project(&remote, "greetx", "1.0.0", "fn hi() -> int { return 10; }\n");
    git(&remote, &["init", "-q", "--initial-branch=main"]);
    git(&remote, &["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
    git(&remote, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "v1"]);
    git(&remote, &["tag", "v1.0.0"]);

    let app = c.dir.join("app");
    c.ok(&["new", app.to_str().unwrap()]);
    c.ok_in(&app, &["add", "--path", "../util"]);
    let remote_s = "../remote".to_string();
    let (_, err, code) = c.wlel_in(&app, &["add", "greetx", &remote_s]);
    assert_eq!(code, Some(0), "{err}");
    assert!(app.join(".wlel/deps/greetx").exists(), "clone cache missing");
    assert!(fs::read_to_string(app.join("wlel.lock")).unwrap().contains("greetx"));

    let out = c.ok_in(&app, &["remove", "util"]);
    assert!(out.contains("removed 'util'"), "{out}");
    let manifest = fs::read_to_string(app.join("wlel.toml")).unwrap();
    assert!(!manifest.contains("util ="), "{manifest}");
    assert!(manifest.contains("greetx ="), "other deps must survive: {manifest}");

    let out = c.ok_in(&app, &["remove", "greetx"]);
    assert!(out.contains("removed 'greetx'"), "{out}");
    assert!(!app.join(".wlel/deps/greetx").exists(), "clone cache not deleted");
    let lock = fs::read_to_string(app.join("wlel.lock")).unwrap();
    assert!(!lock.contains("greetx"), "pin not dropped: {lock}");

    // the project still loads with no dependencies at all
    let (out, err, code) = c.wlel_in(&app, &["check"]);
    assert_eq!(code, Some(0), "{out}{err}");
}

#[test]
fn remove_keeps_the_manifest_byte_identical_apart_from_the_dropped_line() {
    let c = Case::new("remove_bytes");
    let before = "# wlel.toml - project manifest\n[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n# my notes\n[deps]\na = { path = \"a\" }\nb = { path = \"b\" }\n";
    c.write("wlel.toml", before);
    c.write("src/main.wlel", "fn main() -> int { return 0; }\n");
    lib_project(&c.dir.join("a"), "a", "0.1.0", "fn fa() -> int { return 1; }\n");
    lib_project(&c.dir.join("b"), "b", "0.1.0", "fn fb() -> int { return 2; }\n");

    c.ok(&["remove", "a"]);
    let after = c.read("wlel.toml");
    let expected = before.replace("a = { path = \"a\" }\n", "");
    assert_eq!(after, expected, "only the dependency line may change");
}

#[test]
fn removing_an_unknown_dependency_is_a_clear_error() {
    let c = Case::new("remove_missing");
    c.write("wlel.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n");
    c.write("src/main.wlel", "fn main() -> int { return 0; }\n");
    let err = c.fail(&["remove", "nope"]);
    assert!(err.contains("'nope' is not a dependency"), "{err}");
    assert!(err.contains("wlel tree"), "must point at `wlel tree`: {err}");
}

// ---------------------------------------------------------------------------
// update
// ---------------------------------------------------------------------------

#[test]
fn update_moves_the_pin_to_a_new_tag_and_is_idempotent() {
    let c = Case::new("update");
    let remote = c.dir.join("remote");
    lib_project(&remote, "greetx", "1.0.0", "fn hi() -> int { return 10; }\n");
    git(&remote, &["init", "-q", "--initial-branch=main"]);
    git(&remote, &["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
    git(&remote, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "v1"]);
    git(&remote, &["tag", "v1.0.0"]);

    let app = c.dir.join("app");
    c.ok(&["new", app.to_str().unwrap()]);
    // a caret requirement: the lock records "req@rev", and update may move
    // within 1.x without changing the requirement
    let remote_s = "../remote@1.0".to_string();
    let (_, err, code) = c.wlel_in(&app, &["add", "greetx", &remote_s]);
    assert_eq!(code, Some(0), "{err}");
    let first = fs::read_to_string(app.join("wlel.lock")).unwrap();

    // a newer tag that still satisfies "1.0" (caret: >=1.0.0 <2.0.0)
    fs::write(
        remote.join("wlel.toml"),
        "[package]\nname = \"greetx\"\nversion = \"1.1.0\"\n",
    )
    .unwrap();
    fs::write(remote.join("src/lib.wlel"), "fn hi() -> int { return 11; }\n").unwrap();
    git(&remote, &["-c", "user.email=t@t", "-c", "user.name=t", "add", "-A"]);
    git(&remote, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "v11"]);
    git(&remote, &["tag", "v1.1.0"]);

    let out = c.ok_in(&app, &["update"]);
    assert!(out.contains("updated 'greetx'"), "{out}");
    let second = fs::read_to_string(app.join("wlel.lock")).unwrap();
    assert_ne!(first, second, "the pin did not move");
    assert!(second.contains("1.0@"), "the requirement must be kept: {second}");

    // a second update is a no-op
    let out = c.ok_in(&app, &["update"]);
    assert!(out.contains("up to date"), "second update said: {out}");

    // and the new revision is what the program compiles against
    fs::write(
        app.join("src/main.wlel"),
        "use std;\nfn main() -> int { std::println_int(hi()); return 0; }\n",
    )
    .unwrap();
    let out = c.ok_in(&app, &["run"]);
    assert_eq!(out.trim(), "11");
}

#[test]
fn update_names_a_missing_dependency() {
    let c = Case::new("update_missing");
    c.write("wlel.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n");
    c.write("src/main.wlel", "fn main() -> int { return 0; }\n");
    let err = c.fail(&["update", "nope"]);
    assert!(err.contains("'nope' is not a dependency"), "{err}");
}

#[test]
fn update_says_a_path_dependency_has_nothing_to_update() {
    let c = Case::new("update_path");
    lib_project(&c.dir.join("util"), "util", "0.1.0", "fn f() -> int { return 1; }\n");
    let app = c.dir.join("app");
    c.ok(&["new", app.to_str().unwrap()]);
    c.ok_in(&app, &["add", "--path", "../util"]);
    let out = c.ok_in(&app, &["update", "util"]);
    assert!(out.contains("path dependency"), "{out}");
    assert!(out.contains("nothing to update"), "{out}");
}

// ---------------------------------------------------------------------------
// clean
// ---------------------------------------------------------------------------

#[test]
fn clean_removes_the_clone_cache_and_keeps_the_lock() {
    let c = Case::new("clean");
    lib_project(&c.dir.join("util"), "util", "0.1.0", "fn f() -> int { return 1; }\n");
    let app = c.dir.join("app");
    c.ok(&["new", app.to_str().unwrap()]);
    c.ok_in(&app, &["add", "--path", "../util"]);
    fs::create_dir_all(app.join(".wlel/deps/util")).unwrap();

    let out = c.ok_in(&app, &["clean"]);
    assert!(out.contains("removed 1 cached clone"), "{out}");
    assert!(!app.join(".wlel").exists(), ".wlel must be gone");
    // a path-only project never gets a lock file: there is nothing to pin
    assert!(!app.join("wlel.lock").exists(), "no git dep, so no lock");
    assert!(app.join("wlel.toml").exists(), "the manifest must survive");

    // the project still checks (a path dep needs no clone)
    let (out, err, code) = c.wlel_in(&app, &["check"]);
    assert_eq!(code, Some(0), "{out}{err}");
}

#[test]
fn the_new_subcommands_refuse_to_run_outside_a_project() {
    let c = Case::new("outside");
    for args in [vec!["tree"], vec!["clean"], vec!["remove", "x"], vec!["update"]] {
        let err = c.fail(&args);
        assert!(
            err.contains("no wlel.toml found"),
            "wlel {args:?} must explain the missing manifest: {err}"
        );
    }
}
