use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, exit};
use wlel::checker::Checker;
use wlel::codegen::gen_program;
use wlel::lexer::Lexer;
use wlel::parser::Parser;

fn usage() -> ! {
    eprintln!("usage:");
    eprintln!("  wlel run   <file.wl> [args...]");
    eprintln!("  wlel build <file.wl> -o <out>");
    exit(1);
}

struct Args {
    mode: Mode,
}

enum Mode {
    Run { file: String, args: Vec<String> },
    Build { file: String, out: String },
}

fn parse_args() -> Args {
    let a: Vec<String> = env::args().skip(1).collect();
    match a.first().map(|s| s.as_str()) {
        Some("run") if a.len() >= 2 => Args {
            mode: Mode::Run { file: a[1].clone(), args: a[2..].to_vec() },
        },
        Some("build") if a.len() == 4 && a[2] == "-o" => Args {
            mode: Mode::Build { file: a[1].clone(), out: a[3].clone() },
        },
        _ => usage(),
    }
}

/// Wlel source -> C source
fn front(file: &str) -> Result<String, String> {
    let src = fs::read_to_string(file)
        .map_err(|e| format!("cannot read {file}: {e}"))?;
    let toks = Lexer::new(&src).tokenize().map_err(|e| format!("{file}:{e}"))?;
    let program = Parser::new(&toks).program().map_err(|e| format!("{file}: syntax error: {e}"))?;
    Checker::check(&program).map_err(|e| format!("{file}: type error: {e}"))?;
    Ok(gen_program(&program))
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

/// compile generated C into a native binary
fn emit_binary(c_src: &str, out: &PathBuf) -> PathBuf {
    let cc = find_cc();
    let c_path = env::temp_dir().join(format!(
        "wlel_{}.c",
        std::process::id()
    ));
    fs::write(&c_path, c_src).expect("write temp c");
    let status = Command::new(cc)
        .args(["-O2", "-o"])
        .arg(out)
        .arg(&c_path)
        .status()
        .expect("spawn cc");
    let _ = fs::remove_file(&c_path);
    if !status.success() {
        eprintln!("wlel: C backend failed");
        exit(1);
    }
    out.clone()
}

fn main() {
    let args = parse_args();
    let (file, _rest) = match &args.mode {
        Mode::Run { file, args } => (file, args),
        Mode::Build { file, .. } => (file, &Vec::new()),
    };
    let c_src = match front(file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("wlel: {e}");
            exit(1);
        }
    };
    match args.mode {
        Mode::Build { out, .. } => {
            emit_binary(&c_src, &PathBuf::from(out));
            println!("ok");
        }
        Mode::Run { args: run_args, .. } => {
            let bin = emit_binary(&c_src, &env::temp_dir().join("wlel_prog"));
            let status = Command::new(bin)
                .args(run_args)
                .status()
                .expect("run binary");
            exit(status.code().unwrap_or(1));
        }
    }
}
