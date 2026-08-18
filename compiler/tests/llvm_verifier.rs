use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn llvm_verifies_complex_generated_programs() {
    if Command::new("llvm-as")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        return;
    }

    let sources = [
        include_str!("fixtures/llvm/compound.land"),
        include_str!("fixtures/llvm/callable_expressions.land"),
        r#"
fn identity<T>(value: T): T { return value; }
fn increment(value: i32): i32 { return value + 1; }
fn apply<T>(value: T, operation: fn(T): T): T { return operation(value); }
fn main(): i32 {
    let operations = [increment, increment];
    let pair = (identity<i32>(20), apply<i32>(21, operations[1]));
    let (left, right) = pair;
    if left < right { return left + right + 1; } else { return 0; }
}
"#,
    ];

    for source in sources {
        let ir = land::compile(source).unwrap();
        let mut verifier = Command::new("llvm-as")
            .args(["-o", "/dev/null", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        verifier
            .stdin
            .take()
            .unwrap()
            .write_all(ir.as_bytes())
            .unwrap();
        let output = verifier.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "llvm-as rejected generated IR: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
