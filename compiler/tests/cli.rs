use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("land-cli-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn source(&self) -> PathBuf {
        let path = self.root.join("main.land");
        fs::write(&path, "fn main(): i32 { return 42; }").unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn landc() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_landc"))
}

#[test]
fn compiles_and_runs_a_native_executable() {
    let fixture = Fixture::new();
    let source = fixture.source();
    let executable = fixture.root.join("answer");

    let compile = Command::new(landc())
        .arg(&source)
        .args(["-o"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "landc failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    assert_eq!(Command::new(executable).status().unwrap().code(), Some(42));
}

#[test]
fn compiles_and_runs_the_modules_example() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/modules/main.land");
    let directory =
        std::env::temp_dir().join(format!("land-modules-example-{}", std::process::id()));
    let executable = directory.join("modules-example");
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();

    let compile = Command::new(landc())
        .arg(&source)
        .args(["-o"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "landc failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    assert_eq!(Command::new(&executable).status().unwrap().code(), Some(42));

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn compiles_and_runs_the_loops_example() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/loops.land");
    let directory = std::env::temp_dir().join(format!("land-loops-example-{}", std::process::id()));
    let executable = directory.join("loops-example");
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();

    let compile = Command::new(landc())
        .arg(&source)
        .args(["-o"])
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "landc failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    assert_eq!(Command::new(&executable).status().unwrap().code(), Some(45));

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn executable_requires_the_native_main_signature() {
    let fixture = Fixture::new();
    let source = fixture.root.join("main.land");
    fs::write(&source, "fn main(): void { return; }").unwrap();
    let output = Command::new(landc())
        .arg(source)
        .args(["-o"])
        .arg(fixture.root.join("invalid"))
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fn main(): i32"));
}

#[test]
fn reports_the_compiler_version() {
    let output = Command::new(landc()).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "landc 0.0.1\n");
}

#[test]
fn requires_a_link_map_for_imported_headers() {
    let fixture = Fixture::new();
    let header = fixture.root.join("native.landh");
    fs::write(&header, "extern \"C\" fn answer(): i32;").unwrap();
    let source = fixture.root.join("main.land");
    fs::write(
        &source,
        "import { answer } from \"./native.landh\"; fn main(): i32 { return answer(); }",
    )
    .unwrap();
    let output = Command::new(landc())
        .arg(source)
        .args(["-o"])
        .arg(fixture.root.join("answer"))
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no link map was provided"), "{stderr}");
}

#[test]
fn rejects_multiple_providers_for_one_header() {
    let fixture = Fixture::new();
    let header = fixture.root.join("native.landh");
    fs::write(&header, "extern \"C\" fn answer(): i32;").unwrap();
    let source = fixture.root.join("main.land");
    fs::write(
        &source,
        "import { answer } from \"./native.landh\"; fn main(): i32 { return answer(); }",
    )
    .unwrap();
    let link_map = fixture.root.join("links.toml");
    fs::write(
        &link_map,
        r#"
[[provider]]
header = "native.landh"
links = ["production.lib"]

[[provider]]
header = "native.landh"
links = ["test.lib"]
"#,
    )
    .unwrap();

    let output = Command::new(landc())
        .arg(source)
        .args(["-o"])
        .arg(fixture.root.join("answer"))
        .args(["--link-map"])
        .arg(link_map)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("multiple providers"), "{stderr}");
}
