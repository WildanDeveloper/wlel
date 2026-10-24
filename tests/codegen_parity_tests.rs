//! The self-hosted code generator must produce C that *behaves* like the Rust
//! compiler's output: same stdout, same exit status.
//!
//! Byte-identical C is the wrong gate — two correct code generators may
//! parenthesise or order declarations differently. Behaviour is the contract,
//! so each case is compiled twice and run: once through `wlel build`, once
//! through the self-hosted `wgen` plus the system C compiler.
//!
//! The generator consumes the checker's type annotations from the node arena
//! (`Node.ty`), which is why this gate also proves the checker and codegen
//! agree about types.

use std::fs;
use std::path::{Path, PathBuf};
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

/// Build the self-hosted generator once for the whole test binary.
fn generator_binary() -> Option<&'static Path> {
    static BINARY: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    BINARY
        .get_or_init(|| {
            if !have("cc") && !have("gcc") && !have("clang") {
                eprintln!("skipping: no C compiler on PATH");
                return None;
            }
            let out = std::env::temp_dir().join(format!(
                "wlel_selfhost_codegen_{}",
                std::process::id()
            ));
            let _ = fs::remove_file(&out);
            let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
                .args(["build", "selfhost/codegen.wlel", "-O2", "-o"])
                .arg(&out)
                .current_dir(root())
                .output()
                .expect("spawn wlel build");
            assert!(
                build.status.success(),
                "the self-hosted codegen does not build:\n{}{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            );
            Some(out)
        })
        .as_deref()
}

struct Case {
    dir: PathBuf,
    name: String,
}

impl Case {
    fn new(name: &str) -> Case {
        let dir = std::env::temp_dir().join(format!(
            "wlel_cg_{}_{}",
            std::process::id(),
            name.replace(' ', "_")
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        Case { dir, name: name.to_string() }
    }

    fn source(&self) -> PathBuf {
        self.dir.join("main.wlel")
    }

    /// What the Rust compiler produces: build and run.
    fn rust_run(&self, src: &str) -> (String, i32) {
        fs::write(self.source(), src).expect("write source");
        let exe = self.dir.join("rust_bin");
        let build = Command::new(env!("CARGO_BIN_EXE_wlel"))
            .args(["build"])
            .arg(self.source())
            .arg("-O1")
            .arg("-o")
            .arg(&exe)
            .current_dir(root())
            .output()
            .expect("spawn wlel build");
        assert!(
            build.status.success(),
            "the Rust compiler rejected the case:\n{}{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
        run(&exe)
    }

    /// What the self-hosted generator produces: emit C, compile it, run it.
    fn wlel_run(&self, bin: &Path, src: &str) -> (String, i32) {
        fs::write(self.source(), src).expect("write source");
        let c = self.dir.join("gen.c");
        let gen = Command::new(bin)
            .arg(self.source())
            .arg("-o")
            .arg(&c)
            .current_dir(root())
            .output()
            .expect("run the self-hosted codegen");
        assert!(
            gen.status.success(),
            "the self-hosted codegen rejected the case:\n{}",
            String::from_utf8_lossy(&gen.stderr)
        );
        let exe = self.dir.join("wlel_bin");
        let cc = if have("cc") { "cc" } else { "gcc" };
        let compile = Command::new(cc)
            .arg("-O1")
            .arg("-o")
            .arg(&exe)
            .arg(&c)
            .output()
            .expect("spawn the C compiler");
        assert!(
            compile.status.success(),
            "the generated C does not compile for {}:\n{}\n--- C ---\n{}",
            self.name,
            String::from_utf8_lossy(&compile.stderr),
            fs::read_to_string(&c).unwrap_or_default()
        );
        run(&exe)
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn run(exe: &Path) -> (String, i32) {
    let out = Command::new(exe).output().expect("run the built program");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

fn check(bin: &Path, name: &str, src: &str) {
    let c = Case::new(name);
    let expected = c.rust_run(src);
    let actual = c.wlel_run(bin, src);
    assert_eq!(
        actual, expected,
        "case {name:?} behaves differently\n--- source ---\n{src}"
    );
}

#[test]
fn the_self_hosted_codegen_matches_the_rust_one() {
    let Some(bin) = generator_binary() else { return };

    check(
        bin,
        "arithmetic",
        "use std;\n\nfn add(a: int, b: int) -> int {\n    return a + b;\n}\n\nfn main() -> int {\n    x := add(2, 3);\n    std::println_int(x);\n    std::println_int(x * 4 - 1);\n    std::println_int(x / 2);\n    std::println_int(x % 2);\n    return 0;\n}\n",
    );

    check(
        bin,
        "control flow",
        "use std;\n\nfn fib(n: int) -> int {\n    if n < 2 {\n        return n;\n    }\n    return fib(n - 1) + fib(n - 2);\n}\n\nfn main() -> int {\n    i := 0;\n    while i < 10 {\n        if i % 2 == 0 {\n            std::println_int(fib(i));\n        } else {\n            std::println_str(\"odd\");\n        }\n        i += 1;\n    }\n    return 0;\n}\n",
    );

    check(
        bin,
        "structs",
        "use std;\n\nstruct Pt {\n    x: int,\n    y: int\n}\n\nfn dist2(a: Pt, b: Pt) -> int {\n    dx := a.x - b.x;\n    dy := a.y - b.y;\n    return dx * dx + dy * dy;\n}\n\nfn main() -> int {\n    p := Pt { x: 3, y: 4 };\n    q := Pt { x: 0, y: 0 };\n    std::println_int(dist2(p, q));\n    return 0;\n}\n",
    );

    check(
        bin,
        "strings and bools",
        "use std;\n\nfn main() -> int {\n    std::println_str(\"hello\" + \" \" + \"world\");\n    std::println_bool(1 < 2);\n    std::println_bool(2 < 1);\n    std::println_int(std::strlen(\"abcd\"));\n    std::print_str(\"no newline\");\n    std::print_str(\"\\n\");\n    return 0;\n}\n",
    );

    check(
        bin,
        "exit status",
        "use std;\n\nfn main() -> int {\n    std::println_str(\"bye\");\n    return 7;\n}\n",
    );

    check(
        bin,
        "enums and match",
        "use std;\n\nenum Shape {\n    Circle(int),\n    Rect(int, int),\n    Point\n}\n\nfn area(s: Shape) -> int {\n    return match s {\n        Circle(r) => 3 * r * r,\n        Rect(w, h) => w * h,\n        Point => 0,\n    };\n}\n\nfn main() -> int {\n    a := Shape::Circle(2);\n    b := Shape::Rect(3, 4);\n    c := Shape::Point;\n    std::println_int(area(a));\n    std::println_int(area(b));\n    std::println_int(area(c));\n    return 0;\n}\n",
    );

    check(
        bin,
        "range for",
        "use std;\n\nfn main() -> int {\n    total := 0;\n    for i in 0..5 {\n        total += i;\n    }\n    std::println_int(total);\n    return 0;\n}\n",
    );

    check(
        bin,
        "pointers and new",
        "use std;\n\nstruct Node {\n    val: int,\n    next: *Node\n}\n\nfn build(n: int) -> *Node {\n    head := 0 as *Node;\n    i := 0;\n    while i < n {\n        node := new(Node);\n        node.val = i;\n        node.next = head;\n        head = node;\n        i += 1;\n    }\n    return head;\n}\n\nfn sum(h: *Node) -> int {\n    total := 0;\n    p := h;\n    while p as int != 0 {\n        total += p.val;\n        p = p.next;\n    }\n    return total;\n}\n\nfn main() -> int {\n    h := build(1000);\n    std::println_int(sum(h));\n    return 0;\n}\n",
    );

    check(
        bin,
        "narrow widths",
        "use std;\n\nfn main() -> int {\n    a := 200u8;\n    b := 100u8;\n    std::println_int(a as int);\n    std::println_int(b as int);\n    std::println_int((a as int) + (b as int));\n    c := 1.5;\n    std::println_float(c * 2.0);\n    return 0;\n}\n",
    );
}

#[test]
fn the_self_hosted_codegen_reports_throughput() {
    let Some(bin) = generator_binary() else { return };
    let case = Case::new("bench");
    let src = "use std;\n\nstruct Node {\n    val: int,\n    next: *Node\n}\n\nfn build(n: int) -> *Node {\n    head := 0 as *Node;\n    i := 0;\n    while i < n {\n        node := new(Node);\n        node.val = i;\n        node.next = head;\n        head = node;\n        i += 1;\n    }\n    return head;\n}\n\nfn sum(h: *Node) -> int {\n    total := 0;\n    p := h;\n    while p as int != 0 {\n        total += p.val;\n        p = p.next;\n    }\n    return total;\n}\n\nfn main() -> int {\n    std::println_int(sum(build(1000)));\n    return 0;\n}\n";
    fs::write(case.source(), src).expect("write source");
    let run = Command::new(bin)
        .arg(case.source())
        .args(["bench", "20"])
        .current_dir(root())
        .output()
        .expect("run the bench mode");
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let text = String::from_utf8_lossy(&run.stdout);
    assert!(text.contains("20 passes"), "{text}");
    let kb: u64 = text.rsplit(' ').find_map(|w| w.parse().ok()).unwrap_or(0);
    assert!(kb > 500, "codegen throughput regressed: {text}");
}
