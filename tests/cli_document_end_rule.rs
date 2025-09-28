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
fn missing_marker_reports_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("missing.yaml");
    fs::write(&file, "---\nname: value\n").unwrap();

    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-end:\n    level: error\n    present: true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected error: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("missing document end \"...\""),
        "missing message: {output}"
    );
    assert!(
        output.contains("document-end"),
        "rule id missing from output: {output}"
    );
}

#[test]
fn forbidding_marker_flags_explicit_document() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("explicit.yaml");
    fs::write(&file, "---\nname: value\n...\n").unwrap();

    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-end:\n    level: error\n    present: false\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected error: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("found forbidden document end \"...\""),
        "missing forbidden message: {output}"
    );
    assert!(
        output.contains("document-end"),
        "rule id missing from output: {output}"
    );
}

#[test]
fn forbidding_marker_allows_implicit_document() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("implicit.yaml");
    fs::write(&file, "---\nname: value\n").unwrap();

    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-end:\n    level: error\n    present: false\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "implicit end should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}
