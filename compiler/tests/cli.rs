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
