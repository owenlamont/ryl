use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

#[test]
fn invalid_inline_config_causes_exit_2() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("file.yaml");
    fs::write(&file, "key: value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe)
        .arg("-d")
        .arg("extends: missing-config")
        .arg(&file));
    assert_eq!(code, 2, "missing inline extends should exit 2: {err}");
    assert!(
        err.contains("failed to read"),
        "expected config read error: {err}"
    );
}

#[test]
fn empty_inline_config_is_rejected_without_panicking() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("file.yaml");
    fs::write(&file, "key: value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg("-d").arg("").arg(&file));
    assert_eq!(code, 2, "empty config should exit 2, not panic: {err}");
    assert!(
        err.contains("invalid config: not a mapping"),
        "empty config should match yamllint's message: {err}"
    );
}
