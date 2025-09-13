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
fn invalid_project_config_in_dir_causes_exit_2() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir(root.join(".yamllint")).unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg("--list-files").arg(root));
    assert_eq!(code, 2, "expected exit 2: {err}");
    assert!(err.contains("failed to read"));
}

#[test]
fn invalid_project_config_for_explicit_file_causes_exit_2() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir(root.join(".yamllint")).unwrap();
    let f = root.join("a.yaml");
    fs::write(&f, "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg("--list-files").arg(&f));
    assert_eq!(code, 2, "expected exit 2: {err}");
    assert!(err.contains("failed to read"));
}
