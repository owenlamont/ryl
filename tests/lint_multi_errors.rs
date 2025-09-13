use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run ryl");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn multiple_invalid_files_print_multiple_errors() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: [1\n").unwrap();
    fs::write(root.join("b.yaml"), "b: [2\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg(root));
    assert_eq!(code, 1, "expected failure");
    let count = err
        .lines()
        .filter(|l| l.trim().ends_with("(syntax)"))
        .count();
    assert!(
        count >= 2,
        "expected at least two syntax error lines, got: {err}"
    );
}
