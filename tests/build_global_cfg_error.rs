use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn config_file_argument_pointing_to_directory_errors() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir(root.join("cfgdir")).unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let out = Command::new(exe)
        .arg("--list-files")
        .arg("-c")
        .arg(root.join("cfgdir"))
        .arg(root)
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("failed to read"));
}
