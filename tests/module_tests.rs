//! Module namespaces: `module`, qualified access, import lists and the
//! diagnostics that keep them honest.
//!
//! Before this feature every imported file was flattened into one scope, so
//! two files that both defined `helper` were a duplicate-symbol error. These
//! tests pin the replacement: the same name in two modules is fine, a bare
//! reference to it is an ambiguity error that names both, and the qualified
//! form always resolves.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Case {
    dir: PathBuf,
}

impl Case {
    fn new(name: &str) -> Case {
        let dir = std::env::temp_dir().join(format!(
            "wlel_mod_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        Case { dir }
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, text).expect("write");
    }

    fn wlel(&self, args: &[&str]) -> (String, String, Option<i32>) {
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(args)
            .current_dir(&self.dir)
            .output()
            .expect("spawn wlel");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code(),
        )
    }

    fn check(&self, file: &str) -> (String, Option<i32>) {
        let (out, err, code) = self.wlel(&["check", file]);
        (format!("{out}{err}"), code)
    }

    fn run(&self, file: &str) -> (String, Option<i32>) {
        let (out, err, code) = self.wlel(&["run", file]);
        (format!("{out}{err}"), code)
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Two modules that both define `helper`, plus a shared struct name.
fn two_modules(c: &Case) {
    c.write(
        "lib/geom.wlel",
        r#"module geom;

struct Vec2 {
    x: float,
    y: float,
}

fn add(a: Vec2, b: Vec2) -> Vec2 {
    return Vec2 { x: a.x + b.x, y: a.y + b.y };
}

fn helper() -> int {
    return 1;
}
"#,
    );
    c.write(
        "lib/text.wlel",
        r#"module text;

struct Vec2 {
    a: int,
    b: int,
}

fn helper() -> int {
    return 2;
}

fn describe(v: Vec2) -> int {
    return v.a + v.b;
}
"#,
    );
}

#[test]
fn the_same_name_in_two_modules_is_not_a_clash() {
    let c = Case::new("samename");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel";
use "lib/text.wlel";

fn main() -> int {
    a := geom::Vec2 { x: 1.0, y: 2.0 };
    b := geom::Vec2 { x: 3.0, y: 4.0 };
    c := geom::add(a, b);
    t := text::Vec2 { a: 10, b: 20 };
    return text::describe(t) + (c.x as int);
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_eq!(code, Some(0), "{out}");

    // 30 + 4, as the process exit code
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(34), "{out}");
}

#[test]
fn a_bare_name_that_two_modules_share_is_ambiguous() {
    let c = Case::new("ambiguous");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel";
use "lib/text.wlel";

fn main() -> int {
    return helper();
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("ambiguous"), "{out}");
    // the message names both candidates and says how to fix it
    assert!(out.contains("geom::helper"), "{out}");
    assert!(out.contains("text::helper"), "{out}");
    assert!(out.contains("qualify"), "{out}");
}

#[test]
fn a_qualified_name_always_resolves_even_when_the_short_one_is_ambiguous() {
    let c = Case::new("qualified");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel";
use "lib/text.wlel";

fn main() -> int {
    return geom::helper() * 10 + text::helper();
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(12), "{out}");
}

#[test]
fn an_import_list_brings_names_into_scope() {
    let c = Case::new("importlist");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel" { add, Vec2 };
use "lib/text.wlel" { helper };

fn main() -> int {
    a := Vec2 { x: 1.0, y: 2.0 };
    c := add(a, a);
    return helper() + (c.y as int);
}
"#,
    );
    // text::helper wins because only it was imported by name
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(6), "{out}");
}

#[test]
fn an_import_list_can_rename() {
    let c = Case::new("importalias");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel" { add as plus, Vec2 };
use "lib/text.wlel" { helper as text_helper };

fn main() -> int {
    a := Vec2 { x: 1.0, y: 2.0 };
    c := plus(a, a);
    return text_helper() + (c.x as int);
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(4), "{out}");
}

#[test]
fn a_module_alias_works() {
    let c = Case::new("modalias");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel" as g;

fn main() -> int {
    a := g::Vec2 { x: 2.0, y: 3.0 };
    return g::helper() + (a.y as int);
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(4), "{out}");
}

#[test]
fn a_glob_import_makes_every_export_visible() {
    let c = Case::new("glob");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel" { * };

fn main() -> int {
    a := Vec2 { x: 1.0, y: 2.0 };
    b := add(a, a);
    return helper() + (b.x as int);
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(3), "{out}");
}

#[test]
fn an_import_list_naming_a_missing_item_is_an_error() {
    let c = Case::new("badimport");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel" { nonexistent };

fn main() -> int {
    return 0;
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("no item 'nonexistent'"), "{out}");
    assert!(out.contains("geom"), "{out}");
}

#[test]
fn a_plain_import_still_allows_the_short_name() {
    let c = Case::new("plainimport");
    c.write(
        "lib/util.wlel",
        "module util;\n\nfn twice(x: int) -> int { return x * 2; }\n",
    );
    c.write(
        "main.wlel",
        r#"use "lib/util.wlel";

fn main() -> int {
    return twice(21);
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(42), "{out}");
}

#[test]
fn a_module_declaration_names_the_namespace() {
    let c = Case::new("modulename");
    // the file is called `helpers.wlel` but declares `module math;`, so the
    // qualified name follows the declaration
    c.write(
        "lib/helpers.wlel",
        "module math;\n\nfn twice(x: int) -> int { return x * 2; }\n",
    );
    c.write(
        "main.wlel",
        r#"use "lib/helpers.wlel";

fn main() -> int {
    return math::twice(4);
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(8), "{out}");
}

#[test]
fn the_file_stem_is_the_default_module_name() {
    let c = Case::new("stemname");
    // no `module` line: `lib/util.wlel` binds `util`
    c.write("lib/util.wlel", "fn twice(x: int) -> int { return x * 2; }\n");
    c.write(
        "main.wlel",
        r#"use "lib/util.wlel";

fn main() -> int {
    return util::twice(5);
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(10), "{out}");
}

#[test]
fn two_files_cannot_declare_the_same_module() {
    let c = Case::new("dupmodule");
    c.write("lib/a.wlel", "module shared;\n\nfn one() -> int { return 1; }\n");
    c.write("lib/b.wlel", "module shared;\n\nfn two() -> int { return 2; }\n");
    c.write(
        "main.wlel",
        r#"use "lib/a.wlel";
use "lib/b.wlel";

fn main() -> int {
    return shared::one();
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("two files declare `module shared;`"), "{out}");
}

#[test]
fn std_and_sys_are_reserved_module_names() {
    let c = Case::new("reservedmod");
    c.write("lib/x.wlel", "module std;\n\nfn one() -> int { return 1; }\n");
    c.write(
        "main.wlel",
        r#"use "lib/x.wlel";

fn main() -> int {
    return 0;
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("reserved"), "{out}");
}

#[test]
fn a_use_inside_a_module_resolves_that_modules_own_imports() {
    let c = Case::new("nested");
    c.write(
        "lib/base.wlel",
        "module base;\n\nfn three() -> int { return 3; }\n",
    );
    c.write(
        "lib/top.wlel",
        r#"module top;

use "base.wlel";

fn four() -> int {
    return base::three() + 1;
}
"#,
    );
    c.write(
        "main.wlel",
        r#"use "lib/top.wlel";

fn main() -> int {
    return top::four();
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(4), "{out}");
}

#[test]
fn module_items_work_as_types_enums_and_methods() {
    let c = Case::new("rich");
    c.write(
        "lib/geom.wlel",
        r#"module geom;

use std;

struct Vec2 {
    x: float,
    y: float,
}

impl Vec2 {
    fn len(self) -> float {
        return std::math::sqrt(self.x * self.x + self.y * self.y);
    }
}

enum Shape {
    Circle(float),
    Point,
}

fn area(s: Shape) -> float {
    return match s {
        Circle(r) => 3.141592653589793 * r * r,
        Point => 0.0,
    };
}

fn twice[T](x: T) -> T {
    return x;
}
"#,
    );
    c.write(
        "main.wlel",
        r#"use std;
use "lib/geom.wlel";

fn main() -> int {
    v := geom::Vec2 { x: 3.0, y: 4.0 };
    std::println_int(v.len() as int);
    std::println_int(geom::area(geom::Circle(1.0)) as int);
    std::println_int(geom::twice[int](9));
    return 0;
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains('5'), "{out}");
    assert!(out.contains('3'), "{out}"); // pi truncated
    assert!(out.contains('9'), "{out}");
}

#[test]
fn tests_in_imported_modules_run() {
    let c = Case::new("modtests");
    c.write(
        "lib/geom.wlel",
        r#"module geom;

struct Vec2 {
    x: float,
    y: float,
}

fn len2(v: Vec2) -> float {
    return v.x * v.x + v.y * v.y;
}

test "len2 of 3,4 is 25" {
    assert_eq(len2(Vec2 { x: 3.0, y: 4.0 }), 25.0);
}
"#,
    );
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel";

fn main() -> int {
    return 0;
}
"#,
    );
    let (out, err, code) = c.wlel(&["test", "main.wlel"]);
    assert_eq!(code, Some(0), "{out}{err}");
    assert!(out.contains("pass: len2 of 3,4 is 25"), "{out}");
}

#[test]
fn a_name_from_another_module_suggests_the_qualified_form() {
    let c = Case::new("suggest");
    two_modules(&c);
    // `text` is loaded, but only its `Vec2` is imported by name, so
    // `describe` is visible only as `text::describe`
    c.write(
        "main.wlel",
        r#"use "lib/geom.wlel";
use "lib/text.wlel" { Vec2 };

fn main() -> int {
    return describe(Vec2 { a: 1, b: 2 });
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("'describe' is in module 'text'"), "{out}");
    assert!(out.contains("text::describe"), "{out}");
}

#[test]
fn the_formatter_keeps_the_module_line_and_import_lists() {
    let c = Case::new("fmt");
    c.write(
        "lib/geom.wlel",
        "module geom;\n\nfn add(a: int) -> int { return a; }\n",
    );
    c.write(
        "main.wlel",
        "module app;\nuse \"lib/geom.wlel\" as g { add, };\nfn main() -> int { return g::add(1); }\n",
    );
    let (out, err, code) = c.wlel(&["fmt", "main.wlel"]);
    assert_eq!(code, Some(0), "{out}{err}");
    let text = fs::read_to_string(c.dir.join("main.wlel")).expect("read");
    assert!(text.starts_with("module app;"), "{text}");
    assert!(text.contains(r#"use "lib/geom.wlel" as g { add };"#), "{text}");
    // fmt is idempotent on its own output
    let (out, err, code) = c.wlel(&["fmt", "-check", "main.wlel"]);
    assert_eq!(code, Some(0), "{out}{err}");
}

#[test]
fn a_project_dependency_is_a_module() {
    let c = Case::new("projdep");
    c.write(
        "geom/wlel.toml",
        "[package]\nname = \"geom\"\nversion = \"0.1.0\"\n",
    );
    c.write(
        "geom/src/lib.wlel",
        r#"struct Vec2 {
    x: float,
    y: float,
}

fn helper() -> int {
    return 1;
}
"#,
    );
    c.write(
        "app/wlel.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[deps]\ngeom = { path = \"../geom\" }\n",
    );
    c.write(
        "app/src/main.wlel",
        r#"use std;
use geom;

fn helper() -> int {
    return 2;
}

fn main() -> int {
    v := geom::Vec2 { x: 3.0, y: 4.0 };
    std::println_int(helper());
    std::println_int(geom::helper());
    std::println_int(v.y as int);
    return 0;
}
"#,
    );
    let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
        .arg("run")
        .current_dir(c.dir.join("app"))
        .output()
        .expect("spawn wlel");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // the program's own helper and the dependency's never collide
    assert!(stdout.contains('2'), "{stdout}");
    assert!(stdout.contains('1'), "{stdout}");
    assert!(stdout.contains('4'), "{stdout}");
}

#[test]
fn a_single_file_program_is_unchanged() {
    let c = Case::new("single");
    c.write(
        "main.wlel",
        r#"use std;

fn helper() -> int {
    return 3;
}

fn main() -> int {
    std::println_int(helper());
    return 0;
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains('3'), "{out}");
}

#[test]
fn module_declaration_must_come_first() {
    let c = Case::new("modorder");
    c.write(
        "lib/x.wlel",
        "fn one() -> int { return 1; }\n\nmodule x;\n",
    );
    c.write(
        "main.wlel",
        r#"use "lib/x.wlel";

fn main() -> int {
    return 0;
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("`module` must come before every other item"), "{out}");
}

#[test]
fn a_file_cannot_declare_module_twice() {
    let c = Case::new("modtwice");
    c.write("lib/x.wlel", "module a;\nmodule b;\n\nfn one() -> int { return 1; }\n");
    c.write(
        "main.wlel",
        r#"use "lib/x.wlel";

fn main() -> int {
    return 0;
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("a file declares `module` once"), "{out}");
}

#[test]
fn the_repository_examples_still_build() {
    // the feature is additive: every shipped example keeps working unchanged
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for name in ["kitchen", "enums", "generics", "collections", "methods"] {
        let path: PathBuf = root.join("examples").join(format!("{name}.wlel"));
        assert!(path.is_file(), "missing example {name}");
        let out = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(["check", path.to_str().unwrap()])
            .current_dir(&root)
            .output()
            .expect("spawn wlel");
        assert!(
            out.status.success(),
            "examples/{name}.wlel no longer type-checks:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn a_module_can_reach_its_own_items_unqualified() {
    let c = Case::new("ownscope");
    c.write(
        "lib/m.wlel",
        r#"module m;

fn inner() -> int {
    return 7;
}

fn outer() -> int {
    return inner() + 1;
}
"#,
    );
    c.write(
        "main.wlel",
        r#"use "lib/m.wlel";

fn main() -> int {
    return m::outer();
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(8), "{out}");
}

#[test]
fn a_module_does_not_see_the_importers_items() {
    let c = Case::new("noimporter");
    c.write(
        "lib/m.wlel",
        "module m;\n\nfn outer() -> int {\n    return importer_only();\n}\n",
    );
    c.write(
        "main.wlel",
        r#"use "lib/m.wlel";

fn importer_only() -> int {
    return 1;
}

fn main() -> int {
    return m::outer();
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("importer_only"), "{out}");
}

#[test]
fn diamond_imports_of_one_module_are_merged_once() {
    let c = Case::new("diamond");
    c.write(
        "lib/base.wlel",
        "module base;\n\nfn five() -> int { return 5; }\n",
    );
    c.write(
        "lib/left.wlel",
        "module left;\n\nuse \"base.wlel\";\n\nfn a() -> int { return base::five(); }\n",
    );
    c.write(
        "lib/right.wlel",
        "module right;\n\nuse \"base.wlel\";\n\nfn b() -> int { return base::five(); }\n",
    );
    c.write(
        "main.wlel",
        r#"use "lib/left.wlel";
use "lib/right.wlel";

fn main() -> int {
    return left::a() + right::b();
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(10), "{out}");
}

#[test]
fn the_language_server_reports_the_same_module_diagnostics() {
    // the LSP and the CLI share the loader; a module error must surface in
    // both, or the editor would show a clean file the compiler rejects
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("wlel_modlsp_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");
    let main = dir.join("main.wlel");
    fs::write(
        &main,
        "use \"x.wlel\";\n\nfn main() -> int {\n    return helper();\n}\n",
    )
    .expect("write main");
    fs::write(
        dir.join("x.wlel"),
        "module x;\n\nfn helper() -> int { return 1; }\n",
    )
    .expect("write x");

    let analysis = wlel::lsp::analyze(
        &fs::read_to_string(&main).expect("read"),
        Some(main.as_path()),
    );
    let _ = &root;
    // a plain import keeps the short name working, so this file is clean
    assert!(
        analysis.diagnostics.iter().all(|d| d.severity != 1),
        "{:?}",
        analysis.diagnostics.iter().map(|d| &d.msg).collect::<Vec<_>>()
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn qualified_types_are_usable_in_annotations() {
    let c = Case::new("typeannot");
    c.write(
        "lib/geom.wlel",
        "module geom;\n\nstruct Vec2 {\n    x: float,\n    y: float,\n}\n",
    );
    c.write(
        "main.wlel",
        r#"use std;
use "lib/geom.wlel";

fn norm(v: geom::Vec2) -> float {
    return v.x * v.x + v.y * v.y;
}

fn main() -> int {
    v := geom::Vec2 { x: 3.0, y: 4.0 };
    let p: *geom::Vec2 = &v;
    std::println_int(norm(*p) as int);
    return 0;
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("25"), "{out}");
}

#[test]
fn a_qualified_name_is_never_mangled_into_invalid_c() {
    // `geom::add` must reach the C backend as a valid identifier
    let c = Case::new("mangle");
    two_modules(&c);
    c.write(
        "main.wlel",
        r#"use std;
use "lib/geom.wlel";

fn main() -> int {
    a := geom::Vec2 { x: 1.0, y: 1.0 };
    b := geom::add(a, a);
    std::println_float(b.y);
    return 0;
}
"#,
    );
    let (out, err, code) = c.wlel(&["build", "main.wlel", "-o", "bin"]);
    assert_eq!(code, Some(0), "{out}{err}");
    let emitted = c.dir.join("bin.c");
    if emitted.is_file() {
        let text = fs::read_to_string(&emitted).expect("read c");
        assert!(!text.contains("geom::"), "unmangled module path reached C");
        assert!(text.contains("geom__add"), "the qualified function is missing");
    }
    let _ = Path::new(&c.dir);
}

// ---- enum variant namespaces ----------------------------------------------

#[test]
fn two_enums_may_share_a_variant_name() {
    let c = Case::new("variantns");
    c.write(
        "main.wlel",
        r#"use std;

enum Tok {
    Ident(string),
    Eof,
}

enum Expr {
    Ident(string),
    Lit(int),
}

fn describe_tok(t: Tok) -> string {
    return match t {
        Tok::Ident(s) => s,
        Tok::Eof => "eof",
    };
}

fn describe_expr(e: Expr) -> string {
    return match e {
        Expr::Ident(s) => s,
        Expr::Lit(n) => std::int_to_str(n),
    };
}

fn main() -> int {
    std::println_str(describe_tok(Tok::Ident("tok")));
    std::println_str(describe_tok(Tok::Eof));
    std::println_str(describe_expr(Expr::Ident("expr")));
    std::println_str(describe_expr(Expr::Lit(7)));
    return 0;
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("tok"), "{out}");
    assert!(out.contains("eof"), "{out}");
    assert!(out.contains("expr"), "{out}");
    assert!(out.contains('7'), "{out}");
}

#[test]
fn a_unique_variant_still_works_without_a_qualifier() {
    let c = Case::new("variantbare");
    c.write(
        "main.wlel",
        r#"use std;

enum Shape { Circle(float), Point }

fn area(s: Shape) -> float {
    return match s {
        Circle(r) => 3.0 * r * r,
        Point => 0.0,
    };
}

fn main() -> int {
    std::println_int(area(Circle(2.0)) as int);
    std::println_int(area(Point) as int);
    return 0;
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("12"), "{out}");
}

#[test]
fn an_ambiguous_bare_variant_names_both_enums() {
    let c = Case::new("variantamb");
    c.write(
        "main.wlel",
        r#"enum A { Same(int) }
enum B { Same(string) }

fn main() -> int {
    x := Same(1);
    return 0;
}
"#,
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("ambiguous"), "{out}");
    assert!(out.contains("A::Same"), "{out}");
    assert!(out.contains("B::Same"), "{out}");
}

#[test]
fn a_variant_reached_through_a_module_stays_qualified() {
    let c = Case::new("variantmodule");
    c.write(
        "lib/shapes.wlel",
        "module shapes;\n\nenum Shape { Circle(float), Point }\n\nfn unit() -> Shape { return Circle(1.0); }\n",
    );
    c.write(
        "main.wlel",
        r#"use std;
use "lib/shapes.wlel";

fn main() -> int {
    s := shapes::Circle(2.0);
    match s {
        shapes::Circle(r) => { std::println_float(r); }
        shapes::Point => { std::println_str("point"); }
    }
    t := shapes::unit();
    match t {
        shapes::Circle(_) => { std::println_str("unit circle"); }
        shapes::Point => {}
    }
    return 0;
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains('2'), "{out}");
    assert!(out.contains("unit circle"), "{out}");
}

// ---- std:: coverage -------------------------------------------------------

#[test]
fn std_reaches_library_functions_as_well_as_builtins() {
    let c = Case::new("stdprefix");
    c.write(
        "main.wlel",
        r#"use std;

fn main() -> int {
    s := std::str_sub("abcdef", 1, 3);
    std::println_str(s);
    std::println_int(std::str_find(s, "c"));
    std::println_str(std::int_to_str(7));
    xs := std::vec_new[int]();
    xs.push(3);
    std::println_int(std::len([1, 2, 3]));
    std::println_str(std::str_trim("  y "));
    std::println_int(std::str_cmp("a", "b"));
    return 0;
}
"#,
    );
    let (out, code) = c.run("main.wlel");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("bcd"), "{out}");
    assert!(out.contains('1'), "{out}");
    assert!(out.contains('7'), "{out}");
    assert!(out.contains('3'), "{out}");
    assert!(out.contains('y'), "{out}");
}

#[test]
fn an_unknown_std_function_is_reported() {
    let c = Case::new("stdunknown");
    c.write(
        "main.wlel",
        "use std;\nfn main() -> int { return std::no_such_fn(); }\n",
    );
    let (out, code) = c.check("main.wlel");
    assert_ne!(code, Some(0), "{out}");
    assert!(out.contains("no_such_fn"), "{out}");
}
