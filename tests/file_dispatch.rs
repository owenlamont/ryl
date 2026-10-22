use std::sync::Arc;

use ryl::cli_support::{LintFile, small_file_batch};
use ryl::config::{SourceKind, YamlLintConfig};

fn file(path: std::path::PathBuf) -> LintFile {
    (
        path,
        std::path::PathBuf::new(),
        Arc::new(YamlLintConfig::default()),
        SourceKind::Yaml,
    )
}

#[test]
fn empty_batch_needs_no_workers() {
    assert!(small_file_batch(&[]));
}

#[test]
fn single_file_dispatch_includes_64_kib_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("boundary.yaml");
    let input = [file(path.clone())];
    std::fs::write(&path, vec![b'\n'; 65_535]).unwrap();
    assert!(small_file_batch(&input));
    std::fs::write(&path, vec![b'\n'; 65_536]).unwrap();
    assert!(small_file_batch(&input));
    std::fs::write(&path, vec![b'\n'; 65_537]).unwrap();
    assert!(!small_file_batch(&input));
}

#[test]
fn unreadable_metadata_keeps_parallel_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!small_file_batch(&[file(dir.path().join("missing.yaml"))]));
}

#[test]
fn multiple_files_keep_parallel_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.yaml");
    let second = dir.path().join("second.yaml");
    std::fs::write(&first, "").unwrap();
    std::fs::write(&second, "").unwrap();
    assert!(!small_file_batch(&[file(first), file(second)]));
}
