use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

#[test]
fn anchors_reports_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("invalid.yaml");
    fs::write(&file, "---\n- *missing\n- &missing value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  anchors: enable\n")
        .arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("found undeclared alias \"missing\""),
        "missing message: {output}"
    );
    assert!(
        output.contains("anchors"),
        "rule id missing from output: {output}"
    );
}

#[test]
fn warning_level_does_not_fail() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("warn.yaml");
    fs::write(&file, "---\n- *missing\n- &missing value\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  anchors:\n    level: warning\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "warnings should not fail: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("warning"),
        "expected warning output: {output}"
    );
}

#[test]
fn duplicate_anchor_reports_error_when_enabled() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("dupe.yaml");
    fs::write(&file, "---\n- &anchor one\n- &anchor two\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  anchors:\n    forbid-duplicated-anchors: true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("found duplicated anchor \"anchor\""),
        "missing duplicate message: {output}"
    );
}

#[test]
fn malformed_anchor_after_undeclared_alias_reports_syntax_error() {
    // An undefined alias is tolerated (reported via the rule), but a malformed
    // anchor after it is a real syntax error: it must surface (not be masked by
    // the tolerated alias), and the alias diagnostics are then suppressed.
    let dir = tempdir().unwrap();
    let file = dir.path().join("masked.yaml");
    fs::write(&file, "- *first\n- & value\n- *second\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  anchors: enable\n")
        .arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(output.contains("syntax"), "expected syntax error: {output}");
    assert!(
        output.contains("2:3"),
        "expected error at the anchor: {output}"
    );
    assert!(
        !output.contains("undeclared alias"),
        "alias diagnostics should be suppressed by the syntax error: {output}"
    );
}

#[test]
fn unused_anchor_reports_error_when_enabled() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("unused.yaml");
    fs::write(&file, "---\n- &anchor value\n- 1\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  anchors:\n    forbid-unused-anchors: true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("found unused anchor \"anchor\""),
        "missing unused message: {output}"
    );
}

#[test]
fn rule_ignore_skips_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "---\n- *missing\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  anchors:\n    ignore:\n      - ignored.yaml\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "ignored file should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn ambiguous_anchor_alias_names_report_error_via_toml() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ambig.yaml");
    fs::write(&file, "---\na: &foo: 42\nm:\n  - *foo:\n").unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        "[rules.anchors]\nforbid-ambiguous-anchor-alias-names = true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("found ambiguous anchor name \"foo:\""),
        "missing anchor message: {output}"
    );
    assert!(
        output.contains("found ambiguous alias name \"foo:\""),
        "missing alias message: {output}"
    );
    assert!(output.contains("2:4"), "missing anchor span: {output}");
    assert!(output.contains("4:5"), "missing alias span: {output}");
    assert!(output.contains("anchors"), "missing rule id: {output}");
}

#[test]
fn ambiguous_anchor_alias_names_rejected_in_yaml_config() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "a: &foo 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  anchors:\n    forbid-ambiguous-anchor-alias-names: true\n")
        .arg(&file));
    assert_eq!(
        code, 2,
        "expected usage error: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("anchors"),
        "expected config-rejection mentioning anchors: {output}"
    );
}

#[test]
fn alias_value_with_only_indent_prefix_is_supported() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("alias.yaml");
    fs::write(&file, "---\nvalue: &anchor literal\nalias:\n  *anchor\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  anchors: enable\n")
        .arg(&file));
    assert_eq!(
        code, 0,
        "alias resolved successfully: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}
