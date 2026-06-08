use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

#[test]
fn unix_type_reports_crlf() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("crlf.yaml");
    fs::write(&file, "key: value\r\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  new-lines:\n    type: unix\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("wrong new line character: expected \\n"),
        "missing expected message: {output}"
    );
    assert!(output.contains("new-lines"), "rule id missing: {output}");
    assert!(output.contains("1:11"), "incorrect column: {output}");
}

#[test]
fn dos_type_reports_lf() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("lf.yaml");
    fs::write(&file, "key: value\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  new-lines:\n    type: dos\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("wrong new line character: expected \\r\\n"),
        "missing expected message: {output}"
    );
    assert!(output.contains("new-lines"), "rule id missing: {output}");
}

#[test]
fn dos_type_accepts_crlf() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ok.yaml");
    fs::write(&file, "key: value\r\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  new-lines:\n    type: dos\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
}

#[test]
fn platform_type_uses_detected_newline() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("platform.yaml");
    let mismatch = if cfg!(windows) {
        "key: value\n"
    } else {
        "key: value\r\n"
    };
    fs::write(&file, mismatch).unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  new-lines:\n    type: platform\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    if cfg!(windows) {
        assert!(
            output.contains("wrong new line character: expected \\r\\n"),
            "windows platform expectation mismatch: {output}"
        );
    } else {
        assert!(
            output.contains("wrong new line character: expected \\n"),
            "unix platform expectation mismatch: {output}"
        );
    }
    assert!(output.contains("new-lines"), "rule id missing: {output}");
}
