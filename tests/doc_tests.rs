//! `wlel doc` tests: doc-comment extraction, markdown shape, the embedded
//! stdlib page, and the CLI (file mode, project mode, `--std`).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn doc(src: &str) -> wlel::doc::ModuleDoc {
    wlel::doc::document_source(src, "m").expect("documents")
}

// ---------------------------------------------------------------------------
// extraction & markdown shape
// ---------------------------------------------------------------------------

#[test]
fn docs_attach_to_following_declarations() {
    let d = doc(
        r#"/// Adds one.
           fn inc(x: int) -> int { return x + 1; }

           /// A wrapper struct.
           struct Box[T] {
               val: T,
           }

           /// Either ok or dead.
           enum Res {
               Fine,
               Dead,
           }

           impl Box[T] {
               /// the wrapped value
               fn get(self) -> T {
                   return self.val;
               }
           }"#,
    );
    let md = &d.markdown;
    assert!(md.contains("# m\n"), "{md}");
    assert!(md.contains("### inc"), "{md}");
    assert!(md.contains("fn inc(x: int) -> int"), "{md}");
    assert!(md.contains("Adds one."), "{md}");
    assert!(md.contains("### Box[T]"), "{md}");
    assert!(md.contains("struct Box[T] {"), "{md}");
    assert!(md.contains("    val: T,"), "{md}");
    assert!(md.contains("A wrapper struct."), "{md}");
    assert!(md.contains("### Res"), "{md}");
    assert!(md.contains("enum Res {"), "{md}");
    assert!(md.contains("    Fine,"), "{md}");
    assert!(md.contains("Either ok or dead."), "{md}");
    // methods group under their type, docs land on the method
    assert!(md.contains("## Methods"), "{md}");
    assert!(md.contains("### Box[T]"), "{md}");
    assert!(md.contains("#### get"), "{md}");
    assert!(md.contains("fn get(self) -> T"), "{md}");
    assert!(md.contains("the wrapped value"), "{md}");
    assert_eq!((d.structs, d.enums, d.funcs, d.methods), (1, 1, 1, 1));
}

#[test]
fn plain_comments_are_not_docs() {
    let d = doc("// a plain comment\nfn f() { }\n");
    assert!(d.markdown.contains("### f"), "{d:?}");
    assert!(!d.markdown.contains("a plain comment"), "{d:?}");
    assert!(d.summary.is_empty());
}

#[test]
fn four_slash_is_not_a_doc_comment() {
    let d = doc("//// section divider\nfn f() { }\n");
    assert!(!d.markdown.contains("section divider"), "{d:?}");
}

#[test]
fn module_description_from_top_block() {
    let d = doc(
        "/// String helpers for the parser.\n\
         ///\n\
         /// Everything allocates in the active arena.\n\
         \n\
         \n\
         fn f() { }\n",
    );
    // the block is not adjacent to `f` (blank line between), so it becomes
    // the module description
    assert!(d.markdown.contains("# m\n\nString helpers for the parser."), "{d:?}");
    assert!(d.summary == "String helpers for the parser.", "{d:?}");
}

#[test]
fn blank_line_below_doc_still_attaches() {
    let d = doc("/// documented\n\nfn f() { }\n");
    assert!(d.markdown.contains("documented"), "{d:?}");
    assert!(d.summary.is_empty(), "attached docs are not module docs: {d:?}");
}

#[test]
fn extern_fn_shows_extern_form() {
    let d = doc("/// C libm square root.\nextern fn sqrt(x: float) -> float;\n");
    assert!(d.markdown.contains("extern fn sqrt(x: float) -> float;"), "{d:?}");
    assert!(d.markdown.contains("C libm square root."), "{d:?}");
    assert_eq!(d.funcs, 1);
}

#[test]
fn undocumented_items_still_listed() {
    let d = doc("fn f() { }\nfn g() { }\n");
    assert!(d.markdown.contains("### f"), "{d:?}");
    assert!(d.markdown.contains("### g"), "{d:?}");
    assert_eq!(d.funcs, 2);
}

#[test]
fn generic_signatures_carry_type_params() {
    let d = doc("fn id[T](x: T) -> T { return x; }\n");
    assert!(d.markdown.contains("fn id[T](x: T) -> T"), "{d:?}");
}

#[test]
fn tests_and_uses_are_not_documented() {
    let d = doc(
        "use std;\ntest \"t\" { assert(true); }\n",
    );
    assert!(!d.markdown.contains("test"), "{d:?}");
    assert!(!d.markdown.contains("use"), "{d:?}");
    assert!(d.uses_std, "{d:?}");
}

#[test]
fn broken_source_is_rejected() {
    let err = wlel::doc::document_source("fn broken( {", "m").unwrap_err();
    assert!(err.contains("expected identifier"), "{err}");
}

#[test]
fn doc_generation_is_deterministic() {
    let src = "/// d\nfn f() { }\n/// e\nfn g(x: int) { }\n";
    let a = doc(src);
    let b = doc(src);
    assert_eq!(a.markdown, b.markdown);
}

#[test]
fn index_lists_modules_with_counts() {
    let m1 = doc("/// one\nfn f() { }\n");
    let std_page = wlel::doc::std_doc();
    let index = wlel::doc::index_markdown("My Project — documentation", &[std_page, m1]);
    assert!(index.contains("# My Project — documentation"), "{index}");
    assert!(index.contains("| [std](std.md) |"), "{index}");
    assert!(index.contains("| [m](m.md) |"), "{index}");
    assert!(index.contains("1 function"), "{index}");
}

// ---------------------------------------------------------------------------
// stdlib page (the graduation criterion: std docs are generated)
// ---------------------------------------------------------------------------

#[test]
fn std_docs_generated() {
    let s = wlel::doc::std_doc();
    assert_eq!(s.name, "std");
    assert_eq!(s.structs, 2, "Vec + HashMap");
    assert_eq!(s.enums, 2, "Result + Option");
    let md = &s.markdown;
    // module description, types with real docs
    assert!(md.contains("# std\n\nThe Wlel standard library"), "{md}");
    assert!(md.contains("### Vec[T]"), "{md}");
    assert!(md.contains("### HashMap[K, V]"), "{md}");
    assert!(md.contains("### Result[T, E]"), "{md}");
    assert!(md.contains("    Ok(T),"), "{md}");
    assert!(md.contains("### Option[T]"), "{md}");
    // free functions and the method layer, with docs
    assert!(md.contains("fn vec_push[T](v: *Vec[T], x: T)"), "{md}");
    assert!(md.contains("fn str_split(s: string, sep: string) -> Vec[string]"), "{md}");
    assert!(md.contains("fn str_cmp(a: string, b: string) -> int"), "{md}");
    assert!(md.contains("fn map_get_or[K, V](m: *HashMap[K, V], key: K, fallback: V) -> V"), "{md}");
    assert!(md.contains("#### unwrap"), "{md}");
    assert!(md.contains("the payload, or `fallback` when this is `None`"), "{md}");
    // compiler builtins documented by hand
    assert!(md.contains("## Builtin functions"), "{md}");
    assert!(md.contains("`std::format(fmt, ...) -> string`"), "{md}");
    assert!(md.contains("`sys::thread(work, data) -> *Thread`"), "{md}");
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// a throwaway directory (removed on drop)
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "wlel_doc_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        TempDir { path: dir }
    }

    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let p = self.path.join(rel);
        fs::create_dir_all(p.parent().unwrap()).expect("mkdir");
        fs::write(&p, content).expect("write");
        p
    }

    fn wlel(&self, args: &[&str]) -> (String, String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("spawn wlel");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn cli_file_mode_writes_module_page() {
    let t = TempDir::new("file_mode");
    t.write(
        "mathutil.wl",
        "/// Doubles the input.\nfn twice(x: int) -> int { return x * 2; }\n",
    );
    let (out, err, code) = t.wlel(&["doc", "mathutil.wl", "-o", "out"]);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    let page = fs::read_to_string(t.path.join("out/mathutil.md")).expect("read page");
    assert!(page.contains("# mathutil"), "{page}");
    assert!(page.contains("Doubles the input."), "{page}");
    assert!(page.contains("fn twice(x: int) -> int"), "{page}");
    // no std usage -> no std page
    assert!(!t.path.join("out/std.md").exists());
    let index = fs::read_to_string(t.path.join("out/index.md")).expect("read index");
    assert!(index.contains("| [mathutil](mathutil.md) |"), "{index}");
}

#[test]
fn cli_file_mode_with_std_emits_stdlib_page() {
    let t = TempDir::new("std_page");
    t.write(
        "app.wl",
        "use std;\n/// entry\nfn main() -> int { return 0; }\n",
    );
    let (_out, err, code) = t.wlel(&["doc", "app.wl", "-o", "out"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(t.path.join("out/std.md").exists());
    let index = fs::read_to_string(t.path.join("out/index.md")).expect("read index");
    assert!(index.contains("| [std](std.md) |"), "{index}");
    assert!(index.contains("| [app](app.md) |"), "{index}");
}

#[test]
fn cli_project_mode_documents_everything() {
    let t = TempDir::new("project_mode");
    t.write(
        "wlel.toml",
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\n",
    );
    t.write(
        "src/lib.wl",
        "use std;\n/// Doubles the input.\nfn twice(x: int) -> int { return x * 2; }\n",
    );
    t.write(
        "src/extra.wl",
        "/// Adds one.\nfn inc(x: int) -> int { return x + 1; }\n",
    );
    let (out, err, code) = t.wlel(&["doc"]);
    assert_eq!(code, Some(0), "stdout: {out}\nstderr: {err}");
    // every .wl under the project gets a page; std always included
    for page in ["docs/std.md", "docs/lib.md", "docs/extra.md", "docs/index.md"] {
        assert!(t.path.join(page).is_file(), "missing {page}");
    }
    let extra = fs::read_to_string(t.path.join("docs/extra.md")).expect("read");
    assert!(extra.contains("Adds one."), "{extra}");
    // docs land in the project dir even when the cwd is elsewhere
    let index = fs::read_to_string(t.path.join("docs/index.md")).expect("read index");
    assert!(index.contains("# mylib — documentation"), "{index}");
    assert!(index.contains("| [std](std.md) |"), "{index}");
    assert!(index.contains("| [lib](lib.md) |"), "{index}");
    assert!(index.contains("| [extra](extra.md) |"), "{index}");
}

#[test]
fn cli_std_flag_works_without_files() {
    let t = TempDir::new("std_only");
    let (_out, err, code) = t.wlel(&["doc", "--std", "-o", "out"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(t.path.join("out/std.md").is_file());
    assert!(t.path.join("out/index.md").is_file());
    let std_page = fs::read_to_string(t.path.join("out/std.md")).expect("read std");
    assert!(std_page.contains("### Result[T, E]"), "{std_page}");
}

#[test]
fn cli_broken_file_fails_without_writing() {
    let t = TempDir::new("broken");
    t.write("bad.wl", "fn broken( {\n");
    let (_out, err, code) = t.wlel(&["doc", "bad.wl", "-o", "out"]);
    assert_ne!(code, Some(0));
    assert!(err.contains("bad.wl"), "{err}");
    assert!(!t.path.join("out").exists(), "nothing may be written on failure");
}

#[test]
fn cli_name_collision_gets_suffixed() {
    let t = TempDir::new("collision");
    t.write("a/util.wl", "fn f() { }\n");
    t.write("b/util.wl", "fn g() { }\n");
    let (_out, err, code) = t.wlel(&["doc", "a/util.wl", "b/util.wl", "-o", "out"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(t.path.join("out/util.md").is_file());
    assert!(t.path.join("out/util-2.md").is_file());
    let second = fs::read_to_string(t.path.join("out/util-2.md")).expect("read");
    assert!(second.contains("### g"), "{second}");
}
