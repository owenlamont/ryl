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
fn yaml_files_filtering_excludes_dir_files() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("b.yaml"), "b: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg("yaml-files: ['*.none']\n")
        .arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.trim().is_empty(), "expected no files listed: {out}");
}

#[test]
fn explicit_file_not_matching_any_kind_is_rejected() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe)
        .arg("-d")
        .arg("yaml-files: ['*.none']\n")
        .arg(root.join("a.yaml")));
    assert_eq!(code, 2, "expected usage error: {err}");
    assert!(err.contains("no source kind matches"), "{err}");
}
