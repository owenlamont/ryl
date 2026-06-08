use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

#[test]
fn list_files_outputs_expected_entries() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.yaml");
    fs::write(&file, "key: value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("--list-files").arg(dir.path()));
    assert_eq!(code, 0, "list-files should succeed: stderr={stderr}");
    assert!(stderr.trim().is_empty(), "unexpected stderr: {stderr}");
    assert!(
        stdout.contains("sample.yaml"),
        "expected stdout to include listed file: {stdout}"
    );
}
