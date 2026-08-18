use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=native/lld_shim.cpp");
    if env::var_os("CARGO_FEATURE_EMBEDDED_BACKEND").is_none() {
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let llvm_config = env::var("LLVM_CONFIG").unwrap_or_else(|_| "llvm-config".to_owned());
    let cxx = env::var("CXX").unwrap_or_else(|_| "c++".to_owned());
    let include = command(&llvm_config, &["--includedir"]);
    let libdir = command(&llvm_config, &["--libdir"]);
    let object = out.join("lld_shim.o");
    run(Command::new(cxx)
        .args(["-std=c++17", "-fno-exceptions", "-fno-rtti", "-c"])
        .arg("native/lld_shim.cpp")
        .arg(format!("-I{include}"))
        .arg("-o")
        .arg(&object));
    let archive = out.join("libland_lld_shim.a");
    run(Command::new("ar").arg("crs").arg(&archive).arg(&object));

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-search=native={libdir}");
    println!("cargo:rustc-link-lib=static=land_lld_shim");
    println!("cargo:rustc-link-lib=static=lldELF");
    println!("cargo:rustc-link-lib=static=lldCommon");
    for flag in command(
        &llvm_config,
        &["--link-static", "--libs", "all", "--system-libs"],
    )
    .split_whitespace()
    {
        if let Some(library) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={library}");
        }
    }
    println!("cargo:rustc-link-lib=stdc++");
}

fn command(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program).args(arguments).output().unwrap();
    assert!(output.status.success(), "{program} failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn run(command: &mut Command) {
    let program = command.get_program().to_owned();
    assert!(command.status().unwrap().success(), "{program:?} failed");
}
