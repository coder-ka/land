use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;

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
    if arguments.as_slice() == ["--version"] {
        println!("landc {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    let Some(input) = arguments.first() else {
        return Err(usage());
    };
    let mut output = None;
    let mut link_map = None;
    let mut index = 1;
    while index < arguments.len() {
        let flag = &arguments[index];
        let value = arguments.get(index + 1).ok_or_else(usage)?;
        if flag == "-o" && output.is_none() {
            output = Some(PathBuf::from(value));
        } else if flag == "--link-map" && link_map.is_none() {
            link_map = Some(PathBuf::from(value));
        } else {
            return Err(usage());
        }
        index += 2;
    }
    let output = output.ok_or_else(usage)?;
    compile_executable(Path::new(input), &output, link_map.as_deref())?;
    Ok(ExitCode::SUCCESS)
}

fn compile_executable(input: &Path, output: &Path, link_map: Option<&Path>) -> Result<(), String> {
    let (ir, headers) = land::compile_executable_with_headers(input)?;
    let links = resolve_links(link_map, &headers)?;
    #[cfg(feature = "embedded-backend")]
    {
        return native_backend::compile(&ir, output, &links);
    }

    #[cfg(not(feature = "embedded-backend"))]
    compile_with_clang(&ir, output, &links)
}

#[cfg(not(feature = "embedded-backend"))]
fn compile_with_clang(ir: &str, output: &Path, links: &[PathBuf]) -> Result<(), String> {
    let mut clang = Command::new("clang")
        .args([OsStr::new("-x"), OsStr::new("ir"), OsStr::new("-o")])
        .arg(output)
        .arg("-")
        .args([OsStr::new("-x"), OsStr::new("none")])
        .args(links)
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
    "usage: landc <input.land> -o <output> [--link-map <file>]\n       landc --version".to_owned()
}

fn resolve_links(link_map: Option<&Path>, headers: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    if headers.is_empty() {
        return Ok(Vec::new());
    }
    let link_map = link_map.ok_or_else(|| {
        format!(
            "no link map was provided for imported header {}",
            headers[0].display()
        )
    })?;
    let source = fs::read_to_string(link_map)
        .map_err(|error| format!("cannot read link map {}: {error}", link_map.display()))?;
    let base = link_map.parent().unwrap_or_else(|| Path::new("."));
    let document: LinkMap = toml::from_str(&source)
        .map_err(|error| format!("invalid link map {}: {error}", link_map.display()))?;
    let mut mappings = std::collections::HashMap::new();
    for provider in document.provider {
        let header = canonicalize_from(base, &provider.header, "header", link_map)?;
        if provider.links.is_empty() {
            return Err(format!(
                "link map {} provider for {} has no links",
                link_map.display(),
                header.display()
            ));
        }
        let links = provider
            .links
            .iter()
            .map(|link| absolute_from(base, link, "link", link_map))
            .collect::<Result<Vec<_>, _>>()?;
        if mappings.insert(header.clone(), links).is_some() {
            return Err(format!(
                "link map {} has multiple providers for {}",
                link_map.display(),
                header.display()
            ));
        }
    }
    let mut links = Vec::new();
    for header in headers {
        let provider = mappings.get(header).ok_or_else(|| {
            format!(
                "link map {} has no implementation for {}",
                link_map.display(),
                header.display()
            )
        })?;
        links.extend(provider.iter().cloned());
    }
    Ok(links)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkMap {
    #[serde(default)]
    provider: Vec<LinkProvider>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkProvider {
    header: String,
    links: Vec<String>,
}

fn canonicalize_from(
    base: &Path,
    value: &str,
    kind: &str,
    link_map: &Path,
) -> Result<PathBuf, String> {
    let path = absolute_from(base, value, kind, link_map)?;
    fs::canonicalize(&path).map_err(|error| {
        format!(
            "link map {} cannot resolve {kind} {}: {error}",
            link_map.display(),
            path.display()
        )
    })
}

fn absolute_from(base: &Path, value: &str, kind: &str, link_map: &Path) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err(format!(
            "link map {} {kind} path cannot be empty",
            link_map.display()
        ));
    }
    let path = PathBuf::from(value);
    Ok(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}
