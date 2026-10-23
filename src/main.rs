use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use wlel::ast::Program;
use wlel::checker::{CheckWarning, Checker};
use wlel::codegen::{gen_program, gen_program_safe, gen_program_tests, gen_program_tests_safe};
use wlel::project::{self, Project};

/// code generation backend: the C transpiler (default) or the experimental
/// QBE IL emitter (`--backend qbe`, subset of the language)
#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    C,
    Qbe,
}

/// compiler version, stamped from Cargo.toml at build time
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Wlel source extension. Kept in one place so a rename is a one-line
/// change; the pre-1.0 extension was abandoned because GitHub Linguist, most
/// editors and Wolfram's own tooling all claim it for Wolfram Language.
const SOURCE_EXT: &str = "wlel";

/// true for `path/to/file.wlel`. Uses `Path::extension` rather than a suffix
/// test so the dependency cache directory (`.wlel/`) never looks like a
/// source file.
fn is_source_file(path: &str) -> bool {
    Path::new(path).extension().and_then(|e| e.to_str()) == Some(SOURCE_EXT)
}

fn print_version() -> ! {
    println!("wlel {VERSION}");
    exit(0);
}

fn usage() -> ! {
    eprintln!("wlel {VERSION}");
    eprintln!("usage:");
    eprintln!("  wlel new   <dir>                          scaffold a new project");
    eprintln!("  wlel add  [<name>] github:user/repo[@x.y] add a git dependency (git-tag registry)");
    eprintln!("  wlel add  [<name>] <git-url>[@x.y]        add a git dependency by URL");
    eprintln!("  wlel add  [<name>] --path <dir>           add a path dependency");
    eprintln!("  wlel run   <file.wlel> [-l <lib>] [-L <dir>] [-- args...]   run a single file (dev checks)");
    eprintln!("  wlel run   [-l <lib>] [-L <dir>] [-- args...]             run the current project (wlel.toml)");
    eprintln!("  wlel build <file.wlel> -o <out> [-O0|-O1|-O2|-O3] [-sanitize] [--emit-c] [-l <lib>] [-L <dir>] [--backend qbe|c]");
    eprintln!("  wlel build [-o <out>] [-O0|-O1|-O2|-O3] [-sanitize] [--emit-c] [-l <lib>] [-L <dir>] [--backend qbe|c]");
    eprintln!("  wlel check <file.wlel> | (project)");
    eprintln!("  wlel test  <file.wlel> | (project) [-l <lib>] [-L <dir>]");
    eprintln!("  wlel fmt   [<file.wlel>...] [-check]        canonical formatter (idempotent)");
    eprintln!("  wlel doc   [<file.wlel>...] [--std] [-o <dir>]  markdown docs from /// comments");
    eprintln!("  wlel lsp                                  language server (LSP over stdio)");
    eprintln!();
    eprintln!("  -l <lib> / -L <dir>  pass linker flags through to cc (FFI, repeatable)");
    eprintln!();
    eprintln!("  wlel --version       print the compiler version");
    exit(1);
}

struct Args {
    mode: Mode,
}

enum Mode {
    New { dir: String },
    /// `wlel add [<name>] <spec> | --path <dir>`: edit the manifest, resolve,
    /// pin the lock
    Add { name: Option<String>, spec: Option<String>, path: Option<String> },
    /// file: None = project mode (find wlel.toml from cwd upward)
    Run { file: Option<String>, args: Vec<String>, link: Vec<String> },
    Build {
        file: Option<String>,
        out: Option<String>,
        opt: u8,
        emit_c: bool,
        sanitize: bool,
        link: Vec<String>,
        backend: Backend,
    },
    Check { file: Option<String> },
    Test { file: Option<String>, link: Vec<String> },
    /// files: empty = all .wlel files in the current project
    Fmt { files: Vec<String>, check: bool },
    /// markdown documentation: `--std` generates the stdlib page alone,
    /// `-o <dir>` overrides the output directory (default `docs`)
    Doc { files: Vec<String>, out: Option<String>, std: bool },
    /// LSP server over stdio
    Lsp,
}

/// repeated `-l <lib>` / `-L <dir>` linker flags (FFI) — returned as single
/// cc arguments (`-lm`, `-Lsome/dir`); a missing or dashed value is a usage
/// error, everything else links verbatim
fn link_flag(a: &[String], i: &mut usize, kind: &str) -> String {
    let v = match a.get(*i + 1) {
        Some(v) if !v.is_empty() && !v.starts_with('-') => v.clone(),
        _ => {
            eprintln!("wlel: {kind} expects a value (e.g. {kind} m)");
            usage()
        }
    };
    *i += 2;
    format!("{kind}{v}")
}

/// The pre-1.0 extension now belongs to Wolfram Language. A stale file would
/// otherwise be reported as a missing project manifest or an unknown flag, so
/// name the rename explicitly and suggest the fix.
fn reject_legacy_extension(a: &[String]) {
    // everything after `--` belongs to the program being run/tested: a Wlel
    // program may legitimately be handed a Wolfram Language file as data
    let end = a.iter().position(|s| s == "--").unwrap_or(a.len());
    for arg in &a[..end] {
        if Path::new(arg).extension().and_then(|e| e.to_str()) == Some("wl") {
            let renamed = format!("{}.wlel", arg.trim_end_matches(".wl"));
            eprintln!(
                "wlel: '{arg}' uses the .wl extension, which now belongs to Wolfram Language"
            );
            eprintln!("wlel: Wlel sources are .wlel since 1.0 — rename it:");
            eprintln!("wlel:     mv {arg} {renamed}");
            exit(1);
        }
    }
}

fn parse_args() -> Args {
    let a: Vec<String> = env::args().skip(1).collect();
    reject_legacy_extension(&a);
    match a.first().map(|s| s.as_str()) {
        Some("--version" | "-V" | "version") if a.len() == 1 => print_version(),
        Some("new") if a.len() == 2 => Args {
            mode: Mode::New { dir: a[1].clone() },
        },
        Some("add") => {
            let mut path: Option<String> = None;
            let mut positional: Vec<String> = Vec::new();
            let mut i = 1;
            while i < a.len() {
                match a[i].as_str() {
                    "--path" if i + 1 < a.len() => {
                        path = Some(a[i + 1].clone());
                        i += 2;
                    }
                    s if s.starts_with('-') => usage(),
                    s => {
                        positional.push(s.to_string());
                        i += 1;
                    }
                }
            }
            let (name, spec): (Option<String>, Option<String>) = match positional.len() {
                0 if path.is_some() => (None, None),
                1 if path.is_some() => (Some(positional.remove(0)), None),
                1 => (None, Some(positional.remove(0))),
                2 if path.is_none() => (Some(positional.remove(0)), Some(positional.remove(0))),
                _ => usage(),
            };
            if spec.is_none() && path.is_none() {
                usage()
            }
            Args { mode: Mode::Add { name, spec, path } }
        }
        Some("run") => {
            if a.len() >= 2 && is_source_file(&a[1]) {
                // `-l`/`-L` before `--` are wlel linker flags; everything
                // else (including all of the `--` tail) is forwarded
                let sep = a.iter().position(|s| s == "--").unwrap_or(a.len());
                let mut link = Vec::new();
                let mut fwd: Vec<String> = Vec::new();
                let mut i = 2;
                while i < sep {
                    match a[i].as_str() {
                        "-l" => link.push(link_flag(&a, &mut i, "-l")),
                        "-L" => link.push(link_flag(&a, &mut i, "-L")),
                        other => {
                            fwd.push(other.to_string());
                            i += 1;
                        }
                    }
                }
                if sep < a.len() {
                    fwd.extend(a[sep + 1..].iter().cloned());
                }
                Args {
                    mode: Mode::Run { file: Some(a[1].clone()), args: fwd, link },
                }
            } else {
                // project mode: same rule, scanning starts after `run`
                let sep = a.iter().position(|s| s == "--").unwrap_or(a.len());
                let mut link = Vec::new();
                let mut fwd: Vec<String> = Vec::new();
                let mut i = 1;
                while i < sep {
                    match a[i].as_str() {
                        "-l" => link.push(link_flag(&a, &mut i, "-l")),
                        "-L" => link.push(link_flag(&a, &mut i, "-L")),
                        other => {
                            fwd.push(other.to_string());
                            i += 1;
                        }
                    }
                }
                if sep < a.len() {
                    fwd.extend(a[sep + 1..].iter().cloned());
                }
                Args { mode: Mode::Run { file: None, args: fwd, link } }
            }
        }
        Some("build") if a.len() >= 2 && is_source_file(&a[1]) => {
            let file = a[1].clone();
            let mut out: Option<String> = None;
            let mut opt = 2u8;
            let mut emit_c = false;
            let mut sanitize = false;
            let mut link = Vec::new();
            let mut backend = Backend::C;
            let mut i = 2;
            while i < a.len() {
                match a[i].as_str() {
                    "-o" if i + 1 < a.len() => {
                        out = Some(a[i + 1].clone());
                        i += 2;
                    }
                    "-O0" | "-O1" | "-O2" | "-O3" => {
                        opt = a[i][2..].parse().unwrap();
                        i += 1;
                    }
                    "-sanitize" | "--sanitize" => {
                        sanitize = true;
                        i += 1;
                    }
                    "--emit-c" => {
                        emit_c = true;
                        i += 1;
                    }
                    "--backend" if i + 1 < a.len() => {
                        backend = parse_backend(&a[i + 1]);
                        i += 2;
                    }
                    "-l" => link.push(link_flag(&a, &mut i, "-l")),
                    "-L" => link.push(link_flag(&a, &mut i, "-L")),
                    _ => usage(),
                }
            }
            if let Some(o) = out {
                Args {
                    mode: Mode::Build { file: Some(file), out: Some(o), opt, emit_c, sanitize, link, backend },
                }
            } else {
                usage()
            }
        }
        Some("build") => {
            // project mode: flags only, output defaults to the package name
            let mut out: Option<String> = None;
            let mut opt = 2u8;
            let mut emit_c = false;
            let mut sanitize = false;
            let mut link = Vec::new();
            let mut backend = Backend::C;
            let mut i = 1;
            while i < a.len() {
                match a[i].as_str() {
                    "-o" if i + 1 < a.len() => {
                        out = Some(a[i + 1].clone());
                        i += 2;
                    }
                    "-O0" | "-O1" | "-O2" | "-O3" => {
                        opt = a[i][2..].parse().unwrap();
                        i += 1;
                    }
                    "-sanitize" | "--sanitize" => {
                        sanitize = true;
                        i += 1;
                    }
                    "--emit-c" => {
                        emit_c = true;
                        i += 1;
                    }
                    "--backend" if i + 1 < a.len() => {
                        backend = parse_backend(&a[i + 1]);
                        i += 2;
                    }
                    "-l" => link.push(link_flag(&a, &mut i, "-l")),
                    "-L" => link.push(link_flag(&a, &mut i, "-L")),
                    _ => usage(),
                }
            }
            Args { mode: Mode::Build { file: None, out, opt, emit_c, sanitize, link, backend } }
        }
        Some("check") if a.len() == 1 => Args { mode: Mode::Check { file: None } },
        Some("check") if a.len() == 2 => Args {
            mode: Mode::Check { file: Some(a[1].clone()) },
        },
        Some("test") => {
            let mut file: Option<String> = None;
            let mut link = Vec::new();
            let mut i = 1;
            while i < a.len() {
                match a[i].as_str() {
                    "-l" => link.push(link_flag(&a, &mut i, "-l")),
                    "-L" => link.push(link_flag(&a, &mut i, "-L")),
                    f if is_source_file(f) && file.is_none() => {
                        file = Some(f.to_string());
                        i += 1;
                    }
                    _ => usage(),
                }
            }
            Args { mode: Mode::Test { file, link } }
        }
        Some("fmt") => {
            let mut files = Vec::new();
            let mut check = false;
            for x in &a[1..] {
                match x.as_str() {
                    "-check" | "--check" => check = true,
                    _ => files.push(x.clone()),
                }
            }
            Args { mode: Mode::Fmt { files, check } }
        }
        Some("doc") => {
            let mut files = Vec::new();
            let mut out: Option<String> = None;
            let mut std = false;
            let mut i = 1;
            while i < a.len() {
                match a[i].as_str() {
                    "-o" if i + 1 < a.len() => {
                        out = Some(a[i + 1].clone());
                        i += 2;
                    }
                    "--std" => {
                        std = true;
                        i += 1;
                    }
                    f if is_source_file(f) => {
                        files.push(f.to_string());
                        i += 1;
                    }
                    _ => usage(),
                }
            }
            Args { mode: Mode::Doc { files, out, std } }
        }
        Some("lsp") if a.len() == 1 => Args { mode: Mode::Lsp },
        _ => usage(),
    }
}


/// Wlel source -> C source (lex, parse, merge imports, type-check, transpile).
/// In test mode the generated C runs the `test` blocks instead of `main`.
/// In safe mode the C carries debug checks (bounds, div-zero, arena
/// poison) — this is dev (`run`/`test`); `build` compiles release C with
/// zero overhead. Returns the C source plus non-fatal warnings (printed,
/// never fatal).
fn front(file: &str, test_mode: bool, safe: bool) -> Result<(String, Vec<CheckWarning>), String> {
    let program = project::load_program(Path::new(file), &mut HashSet::new())?;
    front_prog(program, file, test_mode, safe)
}

/// `wlel check`: load + splice + type-check, no code generation. Emitting C
/// just to throw it away was a third of a large file's check time.
fn check_front(file: &str) -> Result<Vec<CheckWarning>, String> {
    let program = project::load_program(Path::new(file), &mut HashSet::new())?;
    check_prog(program, file)
}

fn check_prog(
    mut program: Program,
    label: &str,
) -> Result<Vec<CheckWarning>, String> {
    if program.uses.iter().any(|u| u.path.is_none()) {
        wlel::stdsrc::splice_std(&mut program);
    }
    Checker::check(&mut program).map_err(|e| format!("{}:{}: {}", label, e.span, e.msg))
}

/// type-check + transpile an already-loaded program. `use std` splices the
/// embedded library in ahead of user code; it then flows through the same
/// check/monomorphization pipeline, so only the Vec/HashMap instantiations a
/// program actually uses are emitted.
fn front_prog(
    mut program: Program,
    label: &str,
    test_mode: bool,
    safe: bool,
) -> Result<(String, Vec<CheckWarning>), String> {
    if program.uses.iter().any(|u| u.path.is_none()) {
        wlel::stdsrc::splice_std(&mut program);
    }
    let warnings =
        Checker::check(&mut program).map_err(|e| format!("{}:{}: {}", label, e.span, e.msg))?;
    let c = match (test_mode, safe) {
        (true, true) => gen_program_tests_safe(&program),
        (true, false) => gen_program_tests(&program),
        (false, true) => gen_program_safe(&program),
        (false, false) => gen_program(&program),
    };
    Ok((c, warnings))
}

fn print_warnings(file: &str, warnings: &[CheckWarning]) {
    for w in warnings {
        eprintln!("{}:{}: warning: {}", file, w.span, w.msg);
    }
}

fn find_cc() -> String {
    // $CC wins (build systems set it), then the usual names. Looking them
    // up on PATH costs a few stat() calls; spawning `cc --version` for the
    // probe cost ~5 ms on every single build.
    if let Some(cc) = env::var_os("CC") {
        let cc = cc.to_string_lossy().trim().to_string();
        if !cc.is_empty() {
            return cc;
        }
    }
    let names = ["cc", "gcc", "clang"];
    let path = env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = env::split_paths(&path).collect();
    for name in names {
        for dir in &dirs {
            let candidate = dir.join(name);
            if is_executable(&candidate) {
                return name.to_string();
            }
        }
    }
    // nothing on PATH: probe anyway so the diagnosis names the real problem
    for cc in names {
        if Command::new(cc).arg("--version").output().is_ok() {
            return cc.to_string();
        }
    }
    eprintln!("wlel: no C compiler found (need cc, gcc or clang)");
    exit(1);
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        // Windows resolves `cc`/`gcc` to .exe/.bat through PATH itself
        p.is_file() || p.with_extension("exe").is_file()
    }
}

/// FNV-1a 64-bit — dependency-free content hash for the run cache
fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Per-user cache directory. A world-writable `/tmp/wlel_cache` let any
/// local user pre-plant a binary at a content-hash path (the hash is
/// public: it is derived from the C source), which `wlel run` would then
/// execute. The cache now lives under the user's cache dir, created
/// 0700 on Unix; `WLEL_CACHE_DIR` overrides it for sandboxes/CI.
fn cache_dir() -> PathBuf {
    let base = if let Some(d) = env::var_os("WLEL_CACHE_DIR") {
        PathBuf::from(d)
    } else if let Some(d) = env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(d).join("wlel")
    } else if cfg!(windows) {
        env::var_os("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join("wlel"))
            .unwrap_or_else(|| env::temp_dir().join("wlel-cache"))
    } else {
        env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".cache").join("wlel"))
            .unwrap_or_else(|| env::temp_dir().join("wlel-cache"))
    };
    let _ = fs::create_dir_all(&base);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // best effort: the directory must not be group/world writable
        let _ = fs::set_permissions(&base, fs::Permissions::from_mode(0o700));
    }
    base
}

/// A scratch file name unique to this process, inside the private cache
/// directory. `pid` alone is guessable; the counter plus the nanosecond
/// clock makes pre-planting a symlink impractical.
fn scratch_path(ext: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    cache_dir().join(format!("scratch-{}-{nanos:x}-{n}.{ext}", std::process::id()))
}

fn parse_backend(v: &str) -> Backend {
    match v {
        "c" => Backend::C,
        "qbe" => Backend::Qbe,
        _ => usage(),
    }
}

/// MinGW gcc appends .exe to an extensionless -o target, so the file on
/// disk is <name>.exe; callers must track/copy/run that exact file
fn with_exe(p: &Path) -> PathBuf {
    if cfg!(windows) && p.extension().is_none() {
        let mut s = p.to_path_buf().into_os_string();
        s.push(".exe");
        PathBuf::from(s)
    } else {
        p.to_path_buf()
    }
}

/// type-check an already-loaded program, returning it with `.ty` annotations
/// filled in (needed by the QBE backend, which reads checker types)
fn check_program(
    mut program: Program,
    label: &str,
) -> (Program, Vec<CheckWarning>) {
    if program.uses.iter().any(|u| u.path.is_none()) {
        wlel::stdsrc::splice_std(&mut program);
    }
    match Checker::check(&mut program) {
        Ok(warnings) => (program, warnings),
        Err(e) => {
            eprintln!("wlel: {label}:{}: {}", e.span, e.msg);
            exit(1);
        }
    }
}

/// compile QBE IL into a native binary: qbe lowers IL -> asm, cc assembles
/// it together with the small runtime translation unit and links. The C
/// *compiler* never sees generated program code — qbe does that work.
fn emit_binary_qbe(il: &str, out: &Path, link: &[String]) -> PathBuf {
    let cc = find_cc();
    // qbe -h exits immediately; spawn success is the presence probe
    if Command::new("qbe").arg("-h").output().is_err() {
        eprintln!("wlel: qbe not found in PATH (apt install qbe / see https://c9x.me/compile/)");
        exit(1);
    }
    let cache_dir = cache_dir();
    let key = fnv64(format!("qbe|{}\n{il}", link.join("\u{1}")).as_bytes());
    let cache_bin = with_exe(&cache_dir.join(format!("{key:016x}")));
    let out = with_exe(out);
    if cache_bin.exists() {
        if out != cache_bin {
            if let Err(e) = fs::copy(&cache_bin, &out) {
                eprintln!(
                    "wlel: cannot write {}: {e} (does its directory exist?)",
                    out.display()
                );
                exit(1);
            }
        }
        return out;
    }
    let _ = fs::create_dir_all(&cache_dir);
    let ssa_path = scratch_path("ssa");
    let s_path = scratch_path("s");
    let rt_path = scratch_path("rt.c");
    if let Err(e) = fs::write(&ssa_path, il) {
        eprintln!("wlel: cannot write {}: {e}", ssa_path.display());
        exit(1);
    }
    if let Err(e) = fs::write(&rt_path, wlel::qbe::RUNTIME_C) {
        eprintln!("wlel: cannot write {}: {e}", rt_path.display());
        exit(1);
    }
    let out_s = Command::new("qbe")
        .arg(&ssa_path)
        .arg("-o")
        .arg(&s_path)
        .output()
        .expect("spawn qbe");
    if !out_s.status.success() {
        eprintln!("wlel: qbe failed:\n{}", String::from_utf8_lossy(&out_s.stderr));
        exit(1);
    }
    let status = Command::new(cc)
        .arg("-O2")
        .arg("-o")
        .arg(&cache_bin)
        .arg(&s_path)
        .arg(&rt_path)
        // linker flags last: archives resolve against the objects above
        .args(link)
        .status()
        .expect("spawn cc");
    let _ = fs::remove_file(&ssa_path);
    let _ = fs::remove_file(&s_path);
    let _ = fs::remove_file(&rt_path);
    if !status.success() {
        eprintln!("wlel: qbe backend link failed");
        exit(1);
    }
    if out != cache_bin {
        if let Err(e) = fs::copy(&cache_bin, &out) {
            eprintln!(
                "wlel: cannot write {}: {e} (does its directory exist?)",
                out.display()
            );
            exit(1);
        }
    }
    out
}

/// compile generated C into a native binary, reusing a cached build when
/// the (cc, opt level, flags, linker flags, C source) tuple is unchanged
fn emit_binary(
    c_src: &str,
    out: &PathBuf,
    opt: u8,
    allow_cache: bool,
    sanitize: bool,
    link: &[String],
) -> PathBuf {
    let cc = find_cc();
    // Wlel documents two's-complement wrap for signed arithmetic; without
    // this flag signed overflow is UB in C and release builds may miscompile
    // `-pthread`: the concurrency runtime (sys::thread/mutex/chan) links
    // against pthreads on POSIX (harmless no-op define on Windows/MinGW)
    let mut flags = "-fwrapv -pthread".to_string();
    if sanitize {
        // AddressSanitizer build (`wlel build -sanitize`)
        flags.push_str(" -fsanitize=address -fno-omit-frame-pointer");
    }
    let cache_dir = cache_dir();
    let key = fnv64(
        format!("{cc}|O{opt}|{flags}|{}\n{c_src}", link.join("\u{1}")).as_bytes(),
    );
    let cache_bin = cache_dir.join(format!("{key:016x}"));
    // MinGW gcc appends .exe to an extensionless -o target, so the file on
    // disk is <name>.exe; track, copy and return that exact file (callers
    // run the returned path)
    let with_exe = |p: &PathBuf| -> PathBuf {
        if cfg!(windows) && p.extension().is_none() {
            let mut s = p.clone().into_os_string();
            s.push(".exe");
            PathBuf::from(s)
        } else {
            p.clone()
        }
    };
    let out = with_exe(out);
    let cache_bin = with_exe(&cache_bin);
    if allow_cache && cache_bin.exists() {
        if out != cache_bin {
            if let Err(e) = fs::copy(&cache_bin, &out) {
                eprintln!(
                    "wlel: cannot write {}: {e} (does its directory exist?)",
                    out.display()
                );
                exit(1);
            }
        }
        return out;
    }
    let c_path = scratch_path("c");
    if let Err(e) = fs::write(&c_path, c_src) {
        eprintln!("wlel: cannot write {}: {e}", c_path.display());
        exit(1);
    }
    let _ = fs::create_dir_all(&cache_dir);
    let status = Command::new(cc)
        .arg(format!("-O{opt}"))
        .args(flags.split_whitespace())
        .arg("-o")
        .arg(&cache_bin)
        .arg(&c_path)
        // libraries AFTER the object: Debian gcc defaults to
        // -Wl,--as-needed, which discards any -l named before the objects
        // that reference it (std::math's log10/sqrt went unresolved)
        .arg("-lm")
        .args(link)
        .status()
        .expect("spawn cc");
    if !status.success() {
        // keep the generated C around: the cc diagnostics name this path,
        // and deleting it first left the user chasing a ghost file
        eprintln!("wlel: C backend failed");
        eprintln!("wlel: generated C kept at {}", c_path.display());
        exit(1);
    }
    let _ = fs::remove_file(&c_path);
    let _ = fs::create_dir_all(&cache_dir);
    if out != cache_bin {
        if let Err(e) = fs::copy(&cache_bin, &out) {
            eprintln!(
                "wlel: cannot write {}: {e} (does its directory exist?)",
                out.display()
            );
            exit(1);
        }
    }
    out
}

/// load the project for the current directory (walks up to find wlel.toml)
fn find_project() -> Project {
    let cwd = match env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("wlel: cannot get working directory: {e}");
            exit(1);
        }
    };
    match project::find_project_dir(&cwd).and_then(|d| project::load_project(&d)) {
        Ok(p) => {
            for w in &p.warnings {
                eprintln!("wlel: warning: {w}");
            }
            p
        }
        Err(e) => {
            eprintln!("wlel: {e}");
            exit(1);
        }
    }
}

/// every .wlel file under `dir`, recursively; hidden directories (`.wlel`,
/// `.git`, ...) are skipped; sorted for deterministic output
fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if p.is_dir() {
            if !name.starts_with('.') {
                collect_source_files(&p, out);
            }
        } else if is_source_file(&name) {
            out.push(p);
        }
    }
}

fn run_fmt(files: &[String], check: bool) -> ! {
    let paths: Vec<PathBuf> = if files.is_empty() {
        let cwd = match env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("wlel: cannot get working directory: {e}");
                exit(1);
            }
        };
        let dir = match project::find_project_dir(&cwd) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("wlel: {e}");
                exit(1);
            }
        };
        let mut found = Vec::new();
        collect_source_files(&dir, &mut found);
        found
    } else {
        files.iter().map(PathBuf::from).collect()
    };
    if paths.is_empty() {
        eprintln!("wlel: no .wlel files to format");
        exit(1);
    }
    let mut changed = 0usize;
    for p in &paths {
        let src = match fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("wlel: cannot read {}: {e}", p.display());
                exit(1);
            }
        };
        match wlel::fmt::format_source(&src) {
            Ok(formatted) => {
                if formatted != src {
                    changed += 1;
                    if check {
                        println!("would format: {}", p.display());
                    } else if fs::write(p, &formatted).is_err() {
                        eprintln!("wlel: cannot write {}", p.display());
                        exit(1);
                    } else {
                        println!("formatted: {}", p.display());
                    }
                }
            }
            Err(e) => {
                // a file that does not parse is never touched
                eprintln!("wlel: {}: {e}", p.display());
                exit(1);
            }
        }
    }
    if check {
        if changed > 0 {
            exit(1);
        }
        println!("all formatted");
    } else if changed > 0 {
        println!("{changed} file(s) formatted");
    } else {
        println!("all formatted");
    }
    exit(0);
}

/// markdown documentation: one page per source module (plus the embedded
/// stdlib page whenever std is part of the surface) and an index. `--std`
/// alone documents just the stdlib; in project mode every `.wlel` under
/// `wlel.toml` is documented (stdlib page included).
fn run_doc(files: &[String], out_dir: Option<String>, std_only: bool) -> ! {
    let cwd = match env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("wlel: cannot get working directory: {e}");
            exit(1);
        }
    };
    let project_root: Option<PathBuf> = if files.is_empty() && !std_only {
        match project::find_project_dir(&cwd) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("wlel: {e}");
                exit(1);
            }
        }
    } else {
        None
    };
    let paths: Vec<PathBuf> = match project_root {
        Some(ref dir) => {
            let mut found = Vec::new();
            collect_source_files(dir, &mut found);
            found
        }
        None => files.iter().map(PathBuf::from).collect(),
    };
    if paths.is_empty() && !std_only {
        eprintln!("wlel: no .wlel files to document");
        exit(1);
    }
    let out_root = match project_root {
        Some(ref dir) => dir.join(out_dir.clone().unwrap_or_else(|| "docs".into())),
        None => cwd.join(out_dir.clone().unwrap_or_else(|| "docs".into())),
    };

    // user modules are collected first, then the stdlib page is prepended
    // when it belongs to the documented surface (always in project mode,
    // with `--std`, or when any documented module declares `use std`)
    let mut user_modules: Vec<wlel::doc::ModuleDoc> = Vec::new();
    let mut used_names: HashSet<String> = HashSet::new();
    let module_name = |p: &Path, used: &mut HashSet<String>| -> String {
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "module".into());
        let mut name = stem.clone();
        let mut n = 2usize;
        while !used.insert(name.clone()) {
            name = format!("{stem}-{n}");
            n += 1;
        }
        name
    };

    for p in &paths {
        let src = match fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("wlel: cannot read {}: {e}", p.display());
                exit(1);
            }
        };
        let name = module_name(p, &mut used_names);
        match wlel::doc::document_source(&src, &name) {
            Ok(doc) => user_modules.push(doc),
            Err(e) => {
                eprintln!("wlel: {}: {e}", p.display());
                exit(1);
            }
        }
    }
    let needs_std = std_only || project_root.is_some() || user_modules.iter().any(|m| m.uses_std);
    let mut modules: Vec<wlel::doc::ModuleDoc> = Vec::new();
    if needs_std {
        // first available name: "std", then std-2, std-3, ... (a user module
        // named std.wlel keeps its name and the stdlib page moves aside)
        let mut std_name = "std".to_string();
        let mut n = 2usize;
        while !used_names.insert(std_name.clone()) {
            std_name = format!("std-{n}");
            n += 1;
        }
        let mut d = wlel::doc::std_doc();
        d.name = std_name;
        modules.push(d);
    }
    modules.extend(user_modules);

    if fs::create_dir_all(&out_root).is_err() {
        eprintln!("wlel: cannot create {}", out_root.display());
        exit(1);
    }
    // module pages are named "<module>.md"; a source file called
    // `index.wlel` would be overwritten by the generated index below, so the
    // index gets the first free name instead (the page keeps its own name)
    let index_name = {
        let mut n = "index".to_string();
        let mut k = 2usize;
        while modules.iter().any(|m| m.name == n) {
            n = format!("index-{k}");
            k += 1;
        }
        n
    };
    for m in &modules {
        let path = out_root.join(format!("{}.md", m.name));
        if let Err(e) = fs::write(&path, &m.markdown) {
            eprintln!("wlel: cannot write {}: {e}", path.display());
            exit(1);
        }
        println!("doc: {}", path.display());
    }
    let title = match project_root {
        Some(ref dir) => {
            // prefer the package name from the manifest; fall back to the
            // directory name
            let name = fs::read_to_string(dir.join(project::MANIFEST_FILE))
                .ok()
                .and_then(|s| project::parse_manifest(&s).ok())
                .map(|m| m.name)
                .or_else(|| {
                    dir.file_name().map(|n| n.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "project".into());
            format!("{name} — documentation")
        }
        None => "Documentation".into(),
    };
    let index = wlel::doc::index_markdown(&title, &modules);
    let index_path = out_root.join(format!("{index_name}.md"));
    if let Err(e) = fs::write(&index_path, &index) {
        eprintln!("wlel: cannot write {}: {e}", index_path.display());
        exit(1);
    }
    println!("doc: {}", index_path.display());
    exit(0);
}

/// `wlel add [<name>] <spec> | --path <dir>`: resolve the dependency (cloning
/// and tag-selection happen first, so a broken add never edits `wlel.toml`),
/// write the `[deps]` line, then reload the project to pin `wlel.lock`
fn run_add(name: Option<String>, spec: Option<String>, path: Option<String>) -> ! {
    let cwd = match env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("wlel: cannot get working directory: {e}");
            exit(1);
        }
    };
    let dir = match project::find_project_dir(&cwd) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("wlel: {e}");
            exit(1);
        }
    };
    let source = match (spec, path) {
        (Some(s), None) => wlel::registry::AddSource::Git(s),
        (None, Some(p)) => wlel::registry::AddSource::Path(p),
        _ => {
            eprintln!("wlel: give a git spec or --path <dir> (not both)");
            usage()
        }
    };
    match wlel::registry::add_dependency(&dir, name, source) {
        Ok((msg, warnings)) => {
            for w in &warnings {
                eprintln!("wlel: warning: {w}");
            }
            println!("{msg}");
            println!("next: wlel run");
        }
        Err(e) => {
            eprintln!("wlel: {e}");
            exit(1);
        }
    }
    exit(0);
}

/// Exit code for a child process. A signal death (SIGSEGV/SIGILL/…) has no
/// exit code; map it to 128+signal like a shell does and say so, instead of
/// silently reporting a plain 1.
fn exit_code_of(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            eprintln!("wlel: process terminated by signal {sig}");
            return 128 + sig;
        }
    }
    status.code().unwrap_or(1)
}

/// Stack reserved for the compiler pipeline. The parser, checker and codegen
/// are recursive walks over the expression tree, bounded by
/// `checker::MAX_EXPR_DEPTH` (256) and `parser::MAX_NESTING` (256). Those
/// limits were chosen for a 2 MiB thread stack; running on a bigger one means
/// a deeply nested (but legal) program cannot overflow even when the user's
/// shell sets `ulimit -s` low.
const WORKER_STACK: usize = 16 * 1024 * 1024;

fn main() {
    let args = parse_args();
    // the LSP is long-lived and the CLI is short-lived: both run on a thread
    // with a predictable stack instead of whatever the shell provided
    let worker = std::thread::Builder::new()
        .name("wlel".into())
        .stack_size(WORKER_STACK)
        .spawn(move || run(args))
        .expect("spawn compiler thread");
    match worker.join() {
        Ok(()) => {}
        // a panic inside the pipeline is already reported on stderr; exit
        // with a failure code instead of the abort signal a panic would give
        Err(_) => exit(101),
    }
}

fn run(args: Args) {
    match &args.mode {
        Mode::New { dir } => match project::scaffold(Path::new(dir)) {
            Ok(p) => {
                println!("created new project in {}", p.display());
                println!("next: cd {dir} && wlel run");
            }
            Err(e) => {
                eprintln!("wlel: {e}");
                exit(1);
            }
        },
        Mode::Add { name, spec, path } => run_add(name.clone(), spec.clone(), path.clone()),
        Mode::Fmt { files, check } => run_fmt(files, *check),
        Mode::Doc { files, out, std } => run_doc(files, out.as_ref().cloned(), *std),
        Mode::Lsp => {
            wlel::lsp::run(std::io::BufReader::new(std::io::stdin()), std::io::stdout().lock());
            exit(0);
        }
        Mode::Check { file } => match file {
            Some(f) => match check_front(f) {
                Ok(warnings) => {
                    print_warnings(f, &warnings);
                    println!("ok: {f} type-checks");
                }
                Err(e) => {
                    eprintln!("wlel: {e}");
                    exit(1);
                }
            },
            None => {
                let p = find_project();
                let label = p.entry.display().to_string();
                match check_prog(p.program, &label) {
                    Ok(warnings) => {
                        print_warnings(&label, &warnings);
                        println!("ok: project '{}' type-checks", p.manifest.name);
                    }
                    Err(e) => {
                        eprintln!("wlel: {e}");
                        exit(1);
                    }
                }
            }
        },
        Mode::Build { file, out, opt, emit_c, sanitize, link, backend } => match file {
            Some(f) => {
                if *backend == Backend::Qbe {
                    let program = match project::load_program(Path::new(f), &mut HashSet::new()) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("wlel: {e}");
                            exit(1);
                        }
                    };
                    let (program, warnings) = check_program(program, f);
                    print_warnings(f, &warnings);
                    let il = match wlel::qbe::gen_program_il(&program) {
                        Ok(il) => il,
                        Err(e) => {
                            eprintln!("wlel: {e}");
                            exit(1);
                        }
                    };
                    let Some(out_s) = out else { usage() };
                    let out_path = PathBuf::from(out_s);
                    if *emit_c {
                        let ssa_path = out_path.with_extension("ssa");
                        fs::write(&ssa_path, &il).expect("write emitted ssa");
                        println!("ssa -> {}", ssa_path.display());
                    }
                    emit_binary_qbe(&il, &out_path, link);
                    println!("ok");
                    return;
                }
                let (c_src, warnings) = match front(f, false, false) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("wlel: {e}");
                        exit(1);
                    }
                };
                print_warnings(f, &warnings);
                let Some(out_s) = out else { usage() };
                let out_path = PathBuf::from(out_s);
                if *emit_c {
                    let c_path = out_path.with_extension("c");
                    if let Err(e) = fs::write(&c_path, &c_src) {
                        eprintln!(
                            "wlel: cannot write {}: {e} (does its directory exist?)",
                            c_path.display()
                        );
                        exit(1);
                    }
                    println!("c -> {}", c_path.display());
                    // `-o out.c` names the C source itself: writing the
                    // binary to the same path would clobber the file we just
                    // emitted (the old behaviour silently replaced the C
                    // with an ELF). A `.c` target means "stop at the source".
                    if c_path == out_path {
                        println!("ok");
                        return;
                    }
                }
                emit_binary(&c_src, &out_path, *opt, true, *sanitize, link);
                println!("ok");
            }
            None => {
                let p = find_project();
                if !p.is_bin {
                    eprintln!(
                        "wlel: project '{}' is a library (no src/main.wlel) — nothing to build",
                        p.manifest.name
                    );
                    exit(1);
                }
                let label = p.entry.display().to_string();
                let out_name =
                    out.clone().unwrap_or_else(|| p.manifest.name.clone());
                let out_path = p.dir.join(&out_name);
                if *backend == Backend::Qbe {
                    let (program, warnings) = check_program(p.program, &label);
                    print_warnings(&label, &warnings);
                    let il = match wlel::qbe::gen_program_il(&program) {
                        Ok(il) => il,
                        Err(e) => {
                            eprintln!("wlel: {e}");
                            exit(1);
                        }
                    };
                    if *emit_c {
                        let ssa_path = out_path.with_extension("ssa");
                        fs::write(&ssa_path, &il).expect("write emitted ssa");
                        println!("ssa -> {}", ssa_path.display());
                    }
                    emit_binary_qbe(&il, &out_path, link);
                    println!("ok");
                    return;
                }
                let (c_src, warnings) = match front_prog(p.program, &label, false, false) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("wlel: {e}");
                        exit(1);
                    }
                };
                print_warnings(&label, &warnings);
                if *emit_c {
                    let c_path = out_path.with_extension("c");
                    if let Err(e) = fs::write(&c_path, &c_src) {
                        eprintln!(
                            "wlel: cannot write {}: {e} (does its directory exist?)",
                            c_path.display()
                        );
                        exit(1);
                    }
                    println!("c -> {}", c_path.display());
                    // `-o out.c` names the C source itself (see the file
                    // build path above): stop at the source, no link step
                    if c_path == out_path {
                        println!("ok");
                        return;
                    }
                }
                emit_binary(&c_src, &out_path, *opt, true, *sanitize, link);
                println!("ok");
            }
        },
        Mode::Run { file, args: run_args, link } => match file {
            Some(f) => {
                let (c_src, warnings) = match front(f, false, true) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("wlel: {e}");
                        exit(1);
                    }
                };
                print_warnings(f, &warnings);
                // pid-suffixed: concurrent wlel processes must not clobber
                // each other's staged binary
                let bin = emit_binary(
                    &c_src,
                    &env::temp_dir().join(format!("wlel_prog_{}", std::process::id())),
                    2,
                    true,
                    false,
                    link,
                );
                let status = Command::new(bin).args(run_args).status().expect("run binary");
                exit(exit_code_of(status));
            }
            None => {
                let p = find_project();
                if !p.is_bin {
                    eprintln!(
                        "wlel: project '{}' is a library (no src/main.wlel) — nothing to run",
                        p.manifest.name
                    );
                    exit(1);
                }
                let label = p.entry.display().to_string();
                let (c_src, warnings) = match front_prog(p.program, &label, false, true) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("wlel: {e}");
                        exit(1);
                    }
                };
                print_warnings(&label, &warnings);
                let bin = emit_binary(
                    &c_src,
                    &env::temp_dir().join(format!("wlel_prog_{}", std::process::id())),
                    2,
                    true,
                    false,
                    link,
                );
                let status =
                    Command::new(bin).args(run_args).status().expect("run binary");
                exit(exit_code_of(status));
            }
        },
        Mode::Test { file, link } => match file {
            Some(f) => {
                let (c_src, warnings) = match front(f, true, true) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("wlel: {e}");
                        exit(1);
                    }
                };
                print_warnings(f, &warnings);
                let bin = emit_binary(
                    &c_src,
                    &env::temp_dir().join(format!("wlel_test_prog_{}", std::process::id())),
                    2,
                    true,
                    false,
                    link,
                );
                let status = Command::new(bin).status().expect("run tests");
                exit(exit_code_of(status));
            }
            None => {
                let p = find_project();
                let label = p.entry.display().to_string();
                let (c_src, warnings) = match front_prog(p.program, &label, true, true) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("wlel: {e}");
                        exit(1);
                    }
                };
                print_warnings(&label, &warnings);
                let bin = emit_binary(
                    &c_src,
                    &env::temp_dir().join(format!("wlel_test_prog_{}", std::process::id())),
                    2,
                    true,
                    false,
                    link,
                );
                let status = Command::new(bin).status().expect("run tests");
                exit(exit_code_of(status));
            }
        },
    }
}
