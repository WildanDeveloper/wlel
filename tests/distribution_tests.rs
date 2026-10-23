//! Distribution: the install script, the package-manager manifests and the
//! documentation site.
//!
//! The scripts are shell and Python, so nothing in a normal `cargo test` would
//! notice them breaking. These tests check the properties that matter and are
//! cheap: POSIX shell syntax, that every placeholder got substituted, that the
//! manifests point at the same release the binary claims, and that the site
//! renders without leaving markdown behind.
//!
//! Tools that may be absent (`sh`, `python3`, `node`) are optional: a missing
//! tool prints a skip line instead of failing.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn version() -> String {
    let manifest = fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");
    manifest
        .lines()
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("version in Cargo.toml")
}

// ---- install.sh -----------------------------------------------------------

#[test]
fn install_script_is_valid_posix_shell() {
    if !have("sh") {
        eprintln!("skipping: sh is not on PATH");
        return;
    }
    let script = root().join("install.sh");
    assert!(script.is_file(), "install.sh is missing");

    let out = Command::new("sh")
        .args(["-n", script.to_str().unwrap()])
        .output()
        .expect("run sh -n");
    assert!(
        out.status.success(),
        "install.sh is not valid POSIX shell:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = fs::read_to_string(&script).expect("read install.sh");
    // the behaviours the documentation promises
    for needle in [
        "WLEL_VERSION",
        "WLEL_INSTALL_DIR",
        "SHA256SUMS",
        "sha256sum",
        "shasum -a 256",
        "releases/latest",
        "set -eu",
    ] {
        assert!(text.contains(needle), "install.sh lost `{needle}`");
    }
    assert!(
        !text.contains("sudo"),
        "install.sh must never run sudo: it installs to ~/.local/bin instead"
    );

    // every platform the release workflow builds must be mapped
    for target in [
        "x86_64",
        "aarch64",
        "unknown-linux-gnu",
        "apple-darwin",
    ] {
        assert!(text.contains(target), "install.sh does not handle {target}");
    }
}

#[test]
fn install_script_names_the_current_archive_layout() {
    let text = fs::read_to_string(root().join("install.sh")).expect("read install.sh");
    // the release workflow produces wlel-<version>-<target>.tar.gz containing
    // wlel-<version>-<target>/wlel; the script must agree or every install 404s
    assert!(text.contains(r#""${BIN}-${plain}-${target}.tar.gz""#));
    assert!(text.contains(r#"${BIN}-${plain}-${target}/${BIN}"#));

    let release = fs::read_to_string(root().join("scripts/release.sh")).expect("release.sh");
    assert!(
        release.contains(r#"name="wlel-$VERSION-$target""#),
        "release.sh and install.sh disagree on the archive layout"
    );
}

// ---- packaging ------------------------------------------------------------

#[test]
fn packaging_script_renders_manifests_for_the_current_version() {
    if !have("sh") {
        eprintln!("skipping: sh is not on PATH");
        return;
    }
    let script = root().join("scripts/packaging.sh");
    let out = Command::new("sh")
        .args(["-n", script.to_str().unwrap()])
        .output()
        .expect("run sh -n");
    assert!(
        out.status.success(),
        "packaging.sh is not valid POSIX shell:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // a synthetic SHA256SUMS in the release's exact format
    let v = version();
    let dir = std::env::temp_dir().join(format!("wlel_pkg_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let mut sums = String::new();
    for (target, hash) in [
        ("x86_64-unknown-linux-gnu", "a".repeat(64)),
        ("aarch64-unknown-linux-gnu", "b".repeat(64)),
        ("x86_64-apple-darwin", "c".repeat(64)),
        ("aarch64-apple-darwin", "d".repeat(64)),
        ("x86_64-pc-windows-msvc", "e".repeat(64)),
    ] {
        sums.push_str(&format!("{hash}  wlel-{v}-{target}.tar.gz\n"));
    }
    let sums_path = dir.join("SHA256SUMS");
    fs::write(&sums_path, &sums).expect("write sums");

    let pkg_out = dir.join("packaging");
    let out = Command::new("sh")
        .arg(script.to_str().unwrap())
        .args([&v, sums_path.to_str().unwrap()])
        .env("WLEL_PACKAGING_OUT", &pkg_out)
        .current_dir(root())
        .output()
        .expect("run packaging.sh");
    assert!(
        out.status.success(),
        "packaging.sh failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let formula = fs::read_to_string(pkg_out.join("homebrew/wlel.rb")).expect("formula");
    let scoop = fs::read_to_string(pkg_out.join("scoop/wlel.json")).expect("manifest");

    // no placeholder survived
    for text in [&formula, &scoop] {
        assert!(!text.contains('@'), "an unsubstituted placeholder remains:\n{text}");
    }

    assert!(formula.contains(&format!("version \"{v}\"")));
    assert!(formula.contains("class Wlel < Formula"));
    assert!(formula.contains("license \"Apache-2.0\""));
    for hash in ["a".repeat(64), "d".repeat(64)] {
        assert!(formula.contains(&hash), "the formula is missing a hash");
    }
    // the test block must be able to fail
    assert!(formula.contains("assert_match \"wlel #{version}\""));

    assert!(scoop.contains(&format!("\"version\": \"{v}\"")));
    assert!(scoop.contains(&"e".repeat(64)));
    assert!(scoop.contains("\"bin\": \"wlel.exe\""));
    // autoupdate keeps $version literal
    assert!(scoop.contains("$version-x86_64-pc-windows-msvc.tar.gz"));
}

// ---- documentation site ---------------------------------------------------

#[test]
fn docsite_markdown_renders() {
    if !have("python3") {
        eprintln!("skipping: python3 is not on PATH");
        return;
    }
    let out = Command::new("python3")
        .args(["scripts/docsite.py", "--check"])
        .current_dir(root())
        .output()
        .expect("run docsite --check");
    assert!(
        out.status.success(),
        "the documentation site has unrendered markdown:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn docsite_builds_every_page() {
    if !have("python3") {
        eprintln!("skipping: python3 is not on PATH");
        return;
    }
    let dir = std::env::temp_dir().join(format!("wlel_site_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);

    let out = Command::new("python3")
        .args(["scripts/docsite.py", dir.to_str().unwrap()])
        .current_dir(root())
        .output()
        .expect("run docsite");
    assert!(
        out.status.success(),
        "docsite failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let mut pages = Vec::new();
    for expected in [
        "index.html",
        "spec.html",
        "editors.html",
        "vscode.html",
        "tree-sitter.html",
        "linguist.html",
        "cross-compile.html",
        "changelog.html",
        "contributing.html",
        "security.html",
        "style.css",
        "assets/logo.png",
        "playground/index.html",
        "playground/app.js",
    ] {
        let path = dir.join(expected);
        assert!(path.is_file(), "the site is missing {expected}");
        pages.push(expected);
    }

    // every page is a complete document with the shared navigation
    for page in pages.iter().filter(|p| p.ends_with(".html")) {
        let text = fs::read_to_string(dir.join(page)).expect("read page");
        assert!(text.starts_with("<!doctype html>"), "{page} is not a document");
        assert!(text.contains("</html>"), "{page} is truncated");
        assert!(text.contains("class=\"brand\""), "{page} has no header");
        assert!(
            !text.contains("```"),
            "{page} contains an unrendered fenced block"
        );
    }

    let spec = fs::read_to_string(dir.join("spec.html")).expect("read spec");
    // the spec's own headings become a sidebar
    assert!(spec.contains("class=\"docs-nav\""), "the spec has no table of contents");
    assert!(spec.contains("<table>"), "the spec's tables did not render");
    assert!(spec.contains("<blockquote>"), "the spec's notes did not render");

    let index = fs::read_to_string(dir.join("index.html")).expect("read index");
    assert!(index.contains("language-wlel"), "code blocks lost their language");
    assert!(index.contains("<table>"), "the README comparison table did not render");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn playground_sources_are_consistent() {
    let html = fs::read_to_string(root().join("playground/index.html")).expect("index.html");
    let js = fs::read_to_string(root().join("playground/app.js")).expect("app.js");

    // the page loads what the script expects
    assert!(html.contains("app.js"), "the page does not load app.js");
    assert!(html.contains("style.css"), "the page does not load style.css");
    assert!(js.contains("wlel.wasm"), "app.js does not fetch the module");
    assert!(js.contains("wlel_analyze"), "app.js does not call the ABI");

    // every export the JS uses must exist in the Rust module
    let lib = fs::read_to_string(root().join("playground/wasm/src/lib.rs")).expect("wasm lib.rs");
    for export in ["wlel_alloc", "wlel_analyze", "wlel_result_ptr", "wlel_dealloc", "wlel_version"] {
        assert!(lib.contains(export), "the wasm module does not export {export}");
        assert!(js.contains(export), "app.js never uses {export}");
    }

    // the wasm crate must depend on the compiler, not reimplement it
    let cargo = fs::read_to_string(root().join("playground/wasm/Cargo.toml")).expect("wasm Cargo.toml");
    assert!(cargo.contains("crate-type = [\"cdylib\"]"));
    assert!(cargo.contains("wlel = { path = \"../..\" }"));
}

#[test]
fn playground_wasm_module_builds() {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("wasm32-unknown-unknown"))
        .unwrap_or(false);
    if !installed {
        eprintln!("skipping: the wasm32-unknown-unknown target is not installed");
        return;
    }

    let out = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--locked",
            "--manifest-path",
            "playground/wasm/Cargo.toml",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .current_dir(root())
        .output()
        .expect("run cargo build for wasm");
    assert!(
        out.status.success(),
        "the playground module does not build:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let wasm = root().join(
        "playground/wasm/target/wasm32-unknown-unknown/release/wlel_playground.wasm",
    );
    assert!(wasm.is_file(), "the wasm module was not produced");
    let size = fs::metadata(&wasm).expect("stat wasm").len();
    assert!(size > 100_000, "the module is suspiciously small: {size} bytes");
    assert!(
        size < 8 * 1024 * 1024,
        "the module is too large for a playground: {size} bytes"
    );
}

#[test]
fn site_script_is_valid_and_wired() {
    if !have("sh") {
        eprintln!("skipping: sh is not on PATH");
        return;
    }
    let script = root().join("scripts/site.sh");
    let out = Command::new("sh")
        .args(["-n", script.to_str().unwrap()])
        .output()
        .expect("run sh -n");
    assert!(
        out.status.success(),
        "scripts/site.sh is not valid POSIX shell:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = fs::read_to_string(&script).expect("read site.sh");
    assert!(text.contains("docsite.py"), "site.sh does not call the renderer");
    assert!(
        text.contains("wasm32-unknown-unknown"),
        "site.sh does not build the playground module"
    );

    let workflow = fs::read_to_string(root().join(".github/workflows/docs.yml"))
        .expect("docs workflow");
    assert!(workflow.contains("scripts/site.sh"), "CI does not build the site");
    assert!(
        workflow.contains("keep_files: true"),
        "publishing must keep the benchmark trend that lives on gh-pages"
    );
}
