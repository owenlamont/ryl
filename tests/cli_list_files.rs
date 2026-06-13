use std::fs;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

#[test]
fn list_files_outputs_expected_entries() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.yaml");
    fs::write(&file, "key: value\n").unwrap();

    // Bound discovery at the tempdir so a stray config in the shared temp root cannot
    // change which files `--list-files` selects.
    let (code, stdout, stderr) =
        run(ryl(dir.path()).arg("--list-files").arg(dir.path()));
    assert_eq!(code, 0, "list-files should succeed: stderr={stderr}");
    assert!(stderr.trim().is_empty(), "unexpected stderr: {stderr}");
    assert!(
        stdout.contains("sample.yaml"),
        "expected stdout to include listed file: {stdout}"
    );
}
