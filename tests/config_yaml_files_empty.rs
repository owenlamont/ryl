mod common;
use common::cli::run;

use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn empty_yaml_files_falls_back_to_extension_filtering() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("b.txt"), "text\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg("yaml-files: []\n")
        .arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.contains("a.yaml"));
    assert!(!out.contains("b.txt"));
}
