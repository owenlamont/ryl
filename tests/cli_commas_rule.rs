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
fn commas_reports_errors() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "---\n[1,2]\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("too few spaces after comma"),
        "missing message: {output}"
    );
    assert!(
        output.contains("commas"),
        "rule id missing from output: {output}"
    );
}

#[test]
fn warning_level_does_not_fail() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("warn.yaml");
    fs::write(&file, "---\n[1,2]\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  commas:\n    level: warning\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "warnings should not fail: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("warning"),
        "expected warning output: {output}"
    );
}

#[test]
fn rule_ignore_skips_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "---\n[1,2]\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  commas:\n    ignore:\n      - ignored.yaml\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "ignored file should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn relaxed_spacing_allows_compact_flow() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("compact.yaml");
    fs::write(&file, "---\n[1,2]\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  commas:\n    max-spaces-before: -1\n    min-spaces-after: 0\n    max-spaces-after: -1\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "relaxed spacing should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}
