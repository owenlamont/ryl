use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

#[test]
fn comments_rule_emits_diagnostics() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "#comment\nkey: value # comment\nvalue: foo #bar\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  comments:\n    require-starting-space: true\n    ignore-shebangs: true\n    min-spaces-from-content: 2\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("check")
        .arg("-c")
        .arg(&config)
        .arg(&file));

    assert_eq!(code, 1, "expected exit 1: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("comments"),
        "missing rule identifier in output: {output}"
    );
    assert!(
        output.contains("missing starting space in comment"),
        "missing starting-space message: {output}"
    );
    assert!(
        output.contains("too few spaces before comment: expected 2"),
        "missing spacing message: {output}"
    );
}

#[test]
fn comments_rule_ignores_shebang_when_enabled() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("shebang.yaml");
    fs::write(&file, "#!/usr/bin/env foo\n").unwrap();

    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        "[lint.rules]\ndocument-start = \"disable\"\ncomments = { require-starting-space = true, ignore-shebangs = true }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("check")
        .arg("-c")
        .arg(&config)
        .arg(&file));

    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn fix_trims_to_max_spaces_but_honours_disable_line() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[lint.rules.comments]\nmax-spaces-from-content = 2\n",
    )
    .unwrap();
    let file = dir.path().join("spaced.yaml");
    let kept = "kept: 1      # ryl disable-line rule:comments\n";
    fs::write(&file, format!("first: value        # comment\n{kept}")).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("check").arg(&file));
    assert_eq!(code, 1, "expected exit 1: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("1:21"),
        "missing over-max position: {output}"
    );
    assert!(
        !output.contains("2:"),
        "disabled line was reported: {output}"
    );

    let (code, stdout, stderr) =
        run(Command::new(exe).arg("check").arg("--fix").arg(&file));
    assert_eq!(code, 0, "expected exit 0: stdout={stdout} stderr={stderr}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        format!("first: value  # comment\n{kept}")
    );
}
