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
fn missing_newline_reports_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("no_newline.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("no new line character at the end of file"),
        "missing rule message: {output}"
    );
    assert!(
        output.contains("new-line-at-end-of-file"),
        "rule id missing: {output}"
    );
}

#[test]
fn newline_present_succeeds() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ok.yaml");
    fs::write(&file, "key: value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg(&file));
    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
}

#[test]
fn warning_level_does_not_fail() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("warn.yaml");
    fs::write(&file, "key: value").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  new-line-at-end-of-file:\n    level: warning\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "warnings should not fail: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("warning"),
        "expected warning output: {output}"
    );
}

#[test]
fn disabled_new_line_rule_allows_success() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("no_newline.yaml");
    fs::write(&file, "key: value").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(&config, "rules:\n  new-line-at-end-of-file: disable\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "disabled rule should pass: stdout={stdout} stderr={stderr}"
    );
}

#[test]
fn no_warnings_flag_suppresses_output() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("warn.yaml");
    fs::write(&file, "key: value").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  new-line-at-end-of-file:\n    level: warning\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--no-warnings")
        .arg("-c")
        .arg(&config)
        .arg(&file));
    assert_eq!(
        code, 0,
        "no-warnings should suppress warning exit: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn strict_mode_with_warning_exits_with_two() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("warn.yaml");
    fs::write(&file, "key: value").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  new-line-at-end-of-file:\n    level: warning\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--strict")
        .arg("-c")
        .arg(&config)
        .arg(&file));
    assert_eq!(
        code, 2,
        "strict mode should return 2 for warnings: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("warning"),
        "expected warning output: {output}"
    );
}
