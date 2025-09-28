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

fn command_output<'a>(stdout: &'a str, stderr: &'a str) -> &'a str {
    if stderr.is_empty() { stdout } else { stderr }
}

#[test]
fn reports_interior_blank_run() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("interior.yaml");
    fs::write(&file, "key: value\n\n\n\nnext: value\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: disable\n  empty-lines:\n    max: 2\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("too many blank lines (3 > 2)"),
        "missing message: {output}"
    );
    assert!(output.contains("empty-lines"), "rule id missing: {output}");
    assert!(output.contains("4:1"), "incorrect location: {output}");
}

#[test]
fn enforces_start_and_end_limits() {
    let dir = tempdir().unwrap();

    let start_file = dir.path().join("start.yaml");
    fs::write(&start_file, "\n\nkey: value\n").unwrap();
    let start_config = dir.path().join("start-config.yml");
    fs::write(
        &start_config,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: disable\n  empty-lines:\n    max: 5\n    max-start: 1\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&start_config)
        .arg(&start_file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("too many blank lines (2 > 1)"),
        "missing start message: {output}"
    );
    assert!(output.contains("2:1"), "incorrect start location: {output}");

    let end_file = dir.path().join("end.yaml");
    fs::write(&end_file, "key: value\n\n\n").unwrap();
    let end_config = dir.path().join("end-config.yml");
    fs::write(
        &end_config,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: disable\n  empty-lines:\n    max: 5\n    max-end: 1\n",
    )
    .unwrap();

    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&end_config).arg(&end_file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("too many blank lines (2 > 1)"),
        "missing end message: {output}"
    );
    assert!(output.contains("3:1"), "incorrect end location: {output}");
}
