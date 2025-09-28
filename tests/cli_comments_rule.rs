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
fn comments_rule_emits_diagnostics() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "#comment\nkey: value # comment\nvalue: foo #bar\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  comments:\n    require-starting-space: true\n    ignore-shebangs: true\n    min-spaces-from-content: 2\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 1, "expected exit 1: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("comments"),
        "missing rule identifier in output: {output}"
    );
    assert!(
        output.contains("missing starting space in comment"),
        "missing starting-space message: {output}"
    );
    assert!(
        output.contains("too few spaces before comment: expected 2"),
        "missing spacing message: {output}"
    );
}

#[test]
fn comments_rule_ignores_shebang_when_enabled() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("shebang.yaml");
    fs::write(&file, "#!/usr/bin/env foo\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  comments:\n    require-starting-space: true\n    ignore-shebangs: true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.is_empty(), "expected no stderr: {stderr}");
}
