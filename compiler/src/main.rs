use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

#[cfg(not(feature = "embedded-backend"))]
use std::ffi::OsStr;
#[cfg(not(feature = "embedded-backend"))]
use std::io::Write;
#[cfg(not(feature = "embedded-backend"))]
use std::process::{Command, Stdio};

#[cfg(feature = "embedded-backend")]
mod native_backend;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("landc: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    compile_command(env::args_os().skip(1).collect())
}

fn compile_command(arguments: Vec<OsString>) -> Result<ExitCode, String> {
    let [input, flag, output] = arguments.as_slice() else {
        return Err(usage());
    };
    if flag != "-o" {
        return Err(usage());
    }
    compile_executable(Path::new(input), Path::new(output))?;
    Ok(ExitCode::SUCCESS)
}

fn compile_executable(input: &Path, output: &Path) -> Result<(), String> {
    let ir = land::compile_executable(input)?;
    #[cfg(feature = "embedded-backend")]
    return native_backend::compile(&ir, output);

    #[cfg(not(feature = "embedded-backend"))]
    compile_with_clang(&ir, output)
}

#[cfg(not(feature = "embedded-backend"))]
fn compile_with_clang(ir: &str, output: &Path) -> Result<(), String> {
    let mut clang = Command::new("clang")
        .args([OsStr::new("-x"), OsStr::new("ir"), OsStr::new("-o")])
        .arg(output)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start the native backend: {error}"))?;
    clang
        .stdin
        .take()
        .unwrap()
        .write_all(ir.as_bytes())
        .map_err(|error| format!("cannot send LLVM IR to the native backend: {error}"))?;
    let result = clang
        .wait_with_output()
        .map_err(|error| format!("native backend failed: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "native backend failed:\n{}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(())
}

fn usage() -> String {
    "usage: landc <input.land> -o <output>".to_owned()
}
