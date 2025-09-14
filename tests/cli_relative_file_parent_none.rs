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
fn relative_file_without_parent_is_handled() {
    let td = tempdir().unwrap();
    let proj = td.path();
    fs::write(proj.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("a.yaml")
        .current_dir(proj));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.lines().any(|l| l.trim() == "a.yaml"));
}
