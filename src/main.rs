use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use wlel::ast::Program;
use wlel::checker::{CheckWarning, Checker};
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::parser::Parser;

fn usage() -> ! {
    eprintln!("usage:");
    eprintln!("  wlel run   <file.wl> [-- args...]");
    eprintln!("  wlel build <file.wl> -o <out> [-O0|-O1|-O2|-O3] [--emit-c]");
    eprintln!("  wlel check <file.wl>");
    exit(1);
}

struct Args {
    mode: Mode,
}

enum Mode {
    Run { file: String, args: Vec<String> },
    Build { file: String, out: String, opt: u8, emit_c: bool },
    Check { file: String },
}

fn parse_args() -> Args {
    let a: Vec<String> = env::args().skip(1).collect();
    match a.first().map(|s| s.as_str()) {
        Some("run") if a.len() >= 2 => {
            // args after `--` are forwarded to the program
            let fwd = if let Some(pos) = a.iter().position(|s| s == "--") {
                a[pos + 1..].to_vec()
            } else {
                a[2..].to_vec()
            };
            Args {
                mode: Mode::Run { file: a[1].clone(), args: fwd },
            }
        }
        Some("build") if a.len() >= 4 => {
            let file = a[1].clone();
            let mut out: Option<String> = None;
            let mut opt = 2u8;
            let mut emit_c = false;
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
                    "--emit-c" => {
                        emit_c = true;
                        i += 1;
                    }
                    _ => usage(),
                }
            }
            if let Some(o) = out {
                Args {
                    mode: Mode::Build { file, out: o, opt, emit_c },
                }
            } else {
                usage()
            }
        }
        Some("check") if a.len() == 2 => Args {
            mode: Mode::Check { file: a[1].clone() },
        },
        _ => usage(),
    }
}

fn load_program(path: &Path, visited: &mut HashSet<PathBuf>) -> Result<Program, String> {
    let canonical = path.canonicalize().map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !visited.insert(canonical.clone()) {
        // already merged (diamond import)
        return Ok(Program { uses: Vec::new(), structs: Vec::new(), funcs: Vec::new() });
    }
    let src = fs::read_to_string(&canonical)
        .map_err(|e| format!("cannot read {}: {e}", canonical.display()))?;
    let toks = Lexer::new(&src)
        .tokenize()
        .map_err(|e| format!("{}:{}:{}: {e}", canonical.display(), e.line, e.col))?;
    let (mut program, parse_errors) = Parser::new(&toks).program();
    if !parse_errors.is_empty() {
        // report every syntax error found, not just the first
        let msg = parse_errors
            .iter()
            .map(|e| format!("{}: syntax error: {e}", canonical.display()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(msg);
    }

    // remember where each function/struct came from, for `#line` diagnostics
    // and unused-import detection
    let file_str = canonical.display().to_string();
    for f in program.funcs.iter_mut() {
        f.file = file_str.clone();
    }
    for s in program.structs.iter_mut() {
        s.file = file_str.clone();
    }

    // recursively merge `use "path.wl";` imports (relative to the importing file)
    let base_dir = canonical.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let mut import_resolved: Vec<Option<String>> = Vec::new();
    let mut structs = Vec::new();
    let mut funcs = Vec::new();
    for u in &program.uses {
        let Some(rel) = &u.path else {
            import_resolved.push(None);
            continue;
        };
        let ip = base_dir.join(rel);
        let sub = load_program(&ip, visited)?;
        // keep the canonical path so the checker can attribute usage
        import_resolved.push(ip.canonicalize().ok().map(|p| p.display().to_string()));
        structs.extend(sub.structs);
        funcs.extend(sub.funcs);
    }
    for (u, resolved) in program.uses.iter_mut().zip(import_resolved) {
        u.resolved = resolved;
    }
    structs.append(&mut program.structs);
    funcs.append(&mut program.funcs);
    program.structs = structs;
    program.funcs = funcs;
    Ok(program)
}

/// Wlel source -> C source (lex, parse, merge imports, type-check, transpile).
/// Returns the C source plus non-fatal warnings (printed, never fatal).
fn front(file: &str) -> Result<(String, Vec<CheckWarning>), String> {
    let mut program = load_program(Path::new(file), &mut HashSet::new())?;
    let warnings = Checker::check(&mut program).map_err(|e| format!("{}:{}: {}", file, e.span, e.msg))?;
    Ok((gen_program(&program), warnings))
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
/// the (cc, opt level, C source) triple is unchanged
fn emit_binary(c_src: &str, out: &PathBuf, opt: u8, allow_cache: bool) -> PathBuf {
    let cc = find_cc();
    // Wlel documents two's-complement wrap for signed arithmetic; without
    // this flag signed overflow is UB in C and release builds may miscompile
    let flags = "-fwrapv";
    let cache_dir = env::temp_dir().join("wlel_cache");
    let key = fnv64(format!("{cc}|O{opt}|{flags}\n{c_src}").as_bytes());
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
        .args([format!("-O{opt}").as_str(), flags, "-o"])
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

fn main() {
    let args = parse_args();
    match &args.mode {
        Mode::Check { file } => {
            match front(file) {
                Ok((_, warnings)) => {
                    print_warnings(file, &warnings);
                    println!("ok: {file} type-checks");
                }
                Err(e) => {
                    eprintln!("wlel: {e}");
                    exit(1);
                }
            }
        }
        Mode::Build { file, out, opt, emit_c } => {
            let (c_src, warnings) = match front(file) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("wlel: {e}");
                    exit(1);
                }
            };
            print_warnings(file, &warnings);
            if *emit_c {
                let c_path = PathBuf::from(out).with_extension("c");
                fs::write(&c_path, &c_src).expect("write emitted c");
                println!("c -> {}", c_path.display());
            }
            emit_binary(&c_src, &PathBuf::from(out), *opt, false);
            println!("ok");
        }
        Mode::Run { file, args: run_args } => {
            let (c_src, warnings) = match front(file) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("wlel: {e}");
                    exit(1);
                }
            };
            print_warnings(file, &warnings);
            let bin = emit_binary(&c_src, &env::temp_dir().join("wlel_prog"), 2, true);
            let status = Command::new(bin)
                .args(run_args)
                .status()
                .expect("run binary");
            exit(status.code().unwrap_or(1));
        }
    }
}
