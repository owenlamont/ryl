use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

#[test]
fn comments_indentation_reports_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "obj:\n # wrong\n  value: 1\n").unwrap();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  comments-indentation: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 1, "expected exit 1: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("comments-indentation"),
        "missing rule id: {output}"
    );
    assert!(
        output.contains("comment not indented like content"),
        "missing message: {output}"
    );
}

#[test]
fn allow_any_open_indent_accepts_open_block_level_via_toml() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("boundary.yaml");
    // The comment sits at the open `items:` level (0), not the sequence content (2).
    fs::write(&file, "items:\n  - one\n# boundary\n  - two\n").unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        "[rules.comments-indentation]\nallow-any-open-indent = true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
}

#[test]
fn allow_any_open_indent_rejected_in_yaml_config() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "items:\n  - one\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  comments-indentation:\n    allow-any-open-indent: true\n")
        .arg(&file));

    assert_eq!(
        code, 2,
        "expected usage error: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("comments-indentation"),
        "expected config-rejection mentioning comments-indentation: {output}"
    );
}

#[test]
fn comments_indentation_allows_aligned_comment() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ok.yaml");
    fs::write(&file, "obj:\n  # ok\n  value: 1\n").unwrap();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  comments-indentation: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.is_empty(), "expected no stderr: {stderr}");
}
