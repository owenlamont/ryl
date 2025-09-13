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
fn config_file_ignores_docs_globally() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("docs/ignored.yaml"), "x: 0\n").unwrap();

    let cfg = root.join("cfg.yml");
    fs::write(&cfg, "ignore: ['docs/**']\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-c")
        .arg(&cfg)
        .arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.contains("a.yaml"));
    assert!(!out.contains("docs/ignored.yaml"));
}

#[test]
fn config_data_yaml_files_only_lists_dot_yamllint_yml() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join(".yamllint.yml"), "rules: {}\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg("yaml-files: ['**/.yamllint.yml']\n")
        .arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.contains(".yamllint.yml"));
    assert!(!out.contains("a.yaml"));
}

#[test]
fn format_strict_no_warnings_are_accepted() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-f")
        .arg("standard")
        .arg("-s")
        .arg("--no-warnings")
        .arg(td.path()));
    assert_eq!(code, 0, "expected success: {err}");
}
