use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn invalid_inline_config_data_causes_exit_2() {
    let td = tempdir().unwrap();
    let proj = td.path();
    fs::write(proj.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let out = Command::new(exe)
        .arg("-d")
        .arg("rules: {")
        .arg("--list-files")
        .arg(proj)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("failed to parse config data"));
}
