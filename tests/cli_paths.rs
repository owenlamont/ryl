use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run ryl");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn write_file(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).expect("write file");
    path
}

#[test]
fn single_dir_with_valid_and_invalid_yaml() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "good.yml", "a: 1\n");
    write_file(dir.path(), "bad.yaml", "a: [1, 2\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg(dir.path()));

    assert_eq!(code, 1, "expected exit code 1 when invalid YAML found");
    assert!(
        err.contains("bad.yaml"),
        "stderr should include bad file name: {err}"
    );

    // Verify two-line, yamllint-style format with location and label
    let mut lines = err.lines().filter(|l| !l.trim().is_empty());
    let file_line = lines.next().unwrap_or("");
    assert!(
        file_line.ends_with("bad.yaml"),
        "first line should be the file path: {file_line}"
    );
    let second = lines.next().unwrap_or("");
    assert!(
        second.starts_with("  "),
        "second line should begin with two spaces: {second}"
    );
    let pos = second.split_whitespace().next().unwrap_or("");
    let mut it = pos.split(':');
    let line_ok = it.next().and_then(|s| s.parse::<usize>().ok()).is_some();
    let col_ok = it.next().and_then(|s| s.parse::<usize>().ok()).is_some();
    assert!(
        line_ok && col_ok,
        "second line should start with L:C position: {second}"
    );
    assert!(
        second.contains(" error    syntax error: "),
        "second line should include error label: {second}"
    );
}

#[test]
fn multiple_files_allowed_and_report_invalid() {
    let dir = tempdir().unwrap();
    let good = write_file(dir.path(), "one.yml", "k: v\n");
    let bad = write_file(dir.path(), "two.yml", "k: [1,\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg(&good).arg(&bad));

    assert_eq!(code, 1);
    assert!(err.contains("two.yml"));
}

#[test]
fn multiple_directories_are_allowed() {
    let d1 = tempdir().unwrap();
    let d2 = tempdir().unwrap();
    // One good in d1, one bad in d2
    write_file(d1.path(), "ok.yml", "a: 1\n");
    write_file(d2.path(), "bad.yml", "a: [1, 2\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg(d1.path()).arg(d2.path()));

    assert_eq!(code, 1);
    assert!(err.contains("bad.yml"));
}

#[test]
fn mixed_dirs_and_files_are_allowed() {
    let dir = tempdir().unwrap();
    write_file(dir.path(), "g.yaml", "a: b\n");
    let bad = write_file(dir.path(), "bad.yaml", "a: [1,\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg(dir.path()).arg(&bad));

    assert_eq!(code, 1);
    assert!(err.contains("bad.yaml"), "{err}");
}

#[test]
fn explicit_file_with_non_yaml_extension_is_parsed() {
    let dir = tempdir().unwrap();
    let file = write_file(dir.path(), "notes.txt", "a: b\n");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg(&file));

    assert_eq!(code, 0, "expected success parsing YAML in .txt file: {err}");
}
