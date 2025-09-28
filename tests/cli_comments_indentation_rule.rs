use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn comments_indentation_reports_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "obj:\n # wrong\n  value: 1\n").unwrap();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  comments-indentation: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 1, "expected exit 1: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("comments-indentation"),
        "missing rule id: {output}"
    );
    assert!(
        output.contains("comment not indented like content"),
        "missing message: {output}"
    );
}

#[test]
fn comments_indentation_allows_aligned_comment() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ok.yaml");
    fs::write(&file, "obj:\n  # ok\n  value: 1\n").unwrap();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  comments-indentation: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.is_empty(), "expected no stderr: {stderr}");
}
