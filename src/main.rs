use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use wlel::ast::Program;
use wlel::checker::{CheckWarning, Checker};
use wlel::codegen::{gen_program, gen_program_safe, gen_program_tests, gen_program_tests_safe};
use wlel::project::{self, Project};

fn usage() -> ! {
    eprintln!("usage:");
    eprintln!("  wlel new   <dir>                          scaffold a new project");
    eprintln!("  wlel run   <file.wl> [-l <lib>] [-L <dir>] [-- args...]   run a single file (dev checks)");
    eprintln!("  wlel run   [-l <lib>] [-L <dir>] [-- args...]             run the current project (wlel.toml)");
    eprintln!("  wlel build <file.wl> -o <out> [-O0|-O1|-O2|-O3] [-sanitize] [--emit-c] [-l <lib>] [-L <dir>]");
    eprintln!("  wlel build [-o <out>] [-O0|-O1|-O2|-O3] [-sanitize] [--emit-c] [-l <lib>] [-L <dir>]");
    eprintln!("  wlel check <file.wl> | (project)");
    eprintln!("  wlel test  <file.wl> | (project) [-l <lib>] [-L <dir>]");
    eprintln!("  wlel fmt   [<file.wl>...] [-check]        canonical formatter (idempotent)");
    eprintln!();
    eprintln!("  -l <lib> / -L <dir>  pass linker flags through to cc (FFI, repeatable)");
    exit(1);
}

struct Args {
    mode: Mode,
}

enum Mode {
    New { dir: String },
    /// file: None = project mode (find wlel.toml from cwd upward)
    Run { file: Option<String>, args: Vec<String>, link: Vec<String> },
    Build {
        file: Option<String>,
        out: Option<String>,
        opt: u8,
        emit_c: bool,
        sanitize: bool,
        link: Vec<String>,
    },
    Check { file: Option<String> },
    Test { file: Option<String>, link: Vec<String> },
    /// files: empty = all .wl files in the current project
    Fmt { files: Vec<String>, check: bool },
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

fn parse_args() -> Args {
    let a: Vec<String> = env::args().skip(1).collect();
    match a.first().map(|s| s.as_str()) {
        Some("new") if a.len() == 2 => Args {
            mode: Mode::New { dir: a[1].clone() },
        },
        Some("run") => {
            if a.len() >= 2 && a[1].ends_with(".wl") {
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
        Some("build") if a.len() >= 2 && a[1].ends_with(".wl") => {
            let file = a[1].clone();
            let mut out: Option<String> = None;
            let mut opt = 2u8;
            let mut emit_c = false;
            let mut sanitize = false;
            let mut link = Vec::new();
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
                    "-l" => link.push(link_flag(&a, &mut i, "-l")),
                    "-L" => link.push(link_flag(&a, &mut i, "-L")),
                    _ => usage(),
                }
            }
            if let Some(o) = out {
                Args {
                    mode: Mode::Build { file: Some(file), out: Some(o), opt, emit_c, sanitize, link },
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
                    "-l" => link.push(link_flag(&a, &mut i, "-l")),
                    "-L" => link.push(link_flag(&a, &mut i, "-L")),
                    _ => usage(),
                }
            }
            Args { mode: Mode::Build { file: None, out, opt, emit_c, sanitize, link } }
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
                    f if f.ends_with(".wl") && file.is_none() => {
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
        let mut std_prog = wlel::stdsrc::parse_std();
        for f in std_prog.funcs.iter_mut() {
            f.file = wlel::stdsrc::STD_FILE.into();
        }
        for s in std_prog.structs.iter_mut() {
            s.file = wlel::stdsrc::STD_FILE.into();
        }
        for e in std_prog.enums.iter_mut() {
            e.file = wlel::stdsrc::STD_FILE.into();
        }
        program.structs.splice(0..0, std_prog.structs);
        program.enums.splice(0..0, std_prog.enums);
        program.funcs.splice(0..0, std_prog.funcs);
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

fn find_cc() -> &'static str {
    for cc in ["cc", "gcc", "clang"] {
        if Command::new(cc).arg("--version").output().is_ok() {
            return cc;
        }
    }
    eprintln!("wlel: no C compiler found (need cc, gcc or clang)");
    exit(1);
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
    // `-lm`: std::math::* wrappers call libm (glibc needs it at link time)
    let mut flags = "-fwrapv -lm".to_string();
    if sanitize {
        // AddressSanitizer build (`wlel build -sanitize`)
        flags.push_str(" -fsanitize=address -fno-omit-frame-pointer");
    }
    let cache_dir = env::temp_dir().join("wlel_cache");
    let key = fnv64(
        format!("{cc}|O{opt}|{flags}|{}\n{c_src}", link.join("\u{1}")).as_bytes(),
    );
    let cache_bin = cache_dir.join(format!("{key:016x}"));
    if allow_cache && cache_bin.exists() {
        if out != &cache_bin {
            let _ = fs::copy(&cache_bin, out);
        }
        return out.clone();
    }
    let c_path = env::temp_dir().join(format!("wlel_{}.c", std::process::id()));
    fs::write(&c_path, c_src).expect("write temp c");
    let _ = fs::create_dir_all(&cache_dir);
    let status = Command::new(cc)
        .arg(format!("-O{opt}"))
        .args(flags.split_whitespace())
        .args(link)
        .arg("-o")
        .arg(&cache_bin)
        .arg(&c_path)
        .status()
        .expect("spawn cc");
    let _ = fs::remove_file(&c_path);
    if !status.success() {
        eprintln!("wlel: C backend failed");
        exit(1);
    }
    let _ = fs::create_dir_all(&cache_dir);
    if out != &cache_bin {
        let _ = fs::copy(&cache_bin, out);
    }
    out.clone()
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
        Ok(p) => p,
        Err(e) => {
            eprintln!("wlel: {e}");
            exit(1);
        }
    }
}

/// every .wl file under `dir`, recursively; hidden directories (`.wlel`,
/// `.git`, ...) are skipped; sorted for deterministic output
fn collect_wl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if p.is_dir() {
            if !name.starts_with('.') {
                collect_wl_files(&p, out);
            }
        } else if name.ends_with(".wl") {
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
        collect_wl_files(&dir, &mut found);
        found
    } else {
        files.iter().map(PathBuf::from).collect()
    };
    if paths.is_empty() {
        eprintln!("wlel: no .wl files to format");
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

fn main() {
    let args = parse_args();
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
        Mode::Fmt { files, check } => run_fmt(files, *check),
        Mode::Check { file } => match file {
            Some(f) => match front(f, false, false) {
                Ok((_, warnings)) => {
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
                match front_prog(p.program, &label, false, false) {
                    Ok((_, warnings)) => {
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
        Mode::Build { file, out, opt, emit_c, sanitize, link } => match file {
            Some(f) => {
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
                    fs::write(&c_path, &c_src).expect("write emitted c");
                    println!("c -> {}", c_path.display());
                }
                emit_binary(&c_src, &out_path, *opt, false, *sanitize, link);
                println!("ok");
            }
            None => {
                let p = find_project();
                if !p.is_bin {
                    eprintln!(
                        "wlel: project '{}' is a library (no src/main.wl) — nothing to build",
                        p.manifest.name
                    );
                    exit(1);
                }
                let label = p.entry.display().to_string();
                let out_name =
                    out.clone().unwrap_or_else(|| p.manifest.name.clone());
                let out_path = p.dir.join(&out_name);
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
                    fs::write(&c_path, &c_src).expect("write emitted c");
                    println!("c -> {}", c_path.display());
                }
                emit_binary(&c_src, &out_path, *opt, false, *sanitize, link);
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
                exit(status.code().unwrap_or(1));
            }
            None => {
                let p = find_project();
                if !p.is_bin {
                    eprintln!(
                        "wlel: project '{}' is a library (no src/main.wl) — nothing to run",
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
                exit(status.code().unwrap_or(1));
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
                exit(status.code().unwrap_or(1));
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
                exit(status.code().unwrap_or(1));
            }
        },
    }
}
