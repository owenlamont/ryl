use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn cli_inline_preset_expands_relaxed() {
    let td = tempdir().unwrap();
    let yaml = td.path().join("file.yaml");
    fs::write(&yaml, "key: value\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let output = Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg("relaxed")
        .arg(td.path())
        .output()
        .expect("run cli");
    assert_eq!(output.status.code(), Some(0));
}
