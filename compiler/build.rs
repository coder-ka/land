use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=native/lld_shim.cpp");
    if env::var_os("CARGO_FEATURE_EMBEDDED_BACKEND").is_none() {
        return;
    }

    let llvm_config = env::var("LLVM_CONFIG").unwrap_or_else(|_| "llvm-config".to_owned());
    let include = command(&llvm_config, &["--includedir"]);
    let libdir = command(&llvm_config, &["--libdir"]);
    let target = env::var("TARGET").unwrap();
    let windows = target.contains("windows");
    let mut shim = cc::Build::new();
    shim.cpp(true).file("native/lld_shim.cpp").include(include);
    if windows {
        shim.flag("/std:c++17").static_crt(true);
    } else {
        shim.flags(["-std=c++17", "-fno-exceptions", "-fno-rtti"]);
    }
    shim.compile("land_lld_shim");

    println!("cargo:rustc-link-search=native={libdir}");
    if windows {
        let dependencies = env::var("LLVM_DEPENDENCY_DIR")
            .expect("LLVM_DEPENDENCY_DIR is required for the Windows embedded backend");
        println!("cargo:rustc-link-search=native={dependencies}");
    }
    println!(
        "cargo:rustc-link-lib=static={}",
        if windows { "lldCOFF" } else { "lldELF" }
    );
    println!("cargo:rustc-link-lib=static=lldCommon");
    let llvm_libraries = if windows {
        command(
            &llvm_config,
            &[
                "--link-static",
                "--libnames",
                "core",
                "irreader",
                "x86codegen",
                "x86asmparser",
                "option",
                "lto",
                "windowsdriver",
                "windowsmanifest",
                "libdriver",
            ],
        )
    } else {
        command(&llvm_config, &["--link-static", "--libnames", "all"])
    };
    for library in llvm_libraries.split_whitespace() {
        link_library(library);
    }
    for library in command(&llvm_config, &["--link-static", "--system-libs"]).split_whitespace() {
        if windows && library.eq_ignore_ascii_case("libxml2s.lib") {
            continue;
        }
        link_library(library);
    }
    if !windows {
        println!("cargo:rustc-link-lib=stdc++");
    } else {
        for library in ["libxml2", "iconv", "charset", "zs"] {
            println!("cargo:rustc-link-lib=static={library}");
        }
        println!("cargo:rustc-link-lib=bcrypt");
    }
}

fn command(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program).args(arguments).output().unwrap();
    assert!(output.status.success(), "{program} failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn link_library(value: &str) {
    let library = value
        .strip_prefix("-l")
        .or_else(|| {
            value
                .strip_prefix("lib")
                .and_then(|name| name.strip_suffix(".a"))
        })
        .or_else(|| value.strip_suffix(".lib"))
        .unwrap_or(value);
    println!("cargo:rustc-link-lib={library}");
}
