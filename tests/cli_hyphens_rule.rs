use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

#[test]
fn hyphens_reports_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "---\n-  item\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  hyphens: enable\n")
        .arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("too many spaces after hyphen"),
        "missing message: {output}"
    );
    assert!(
        output.contains("hyphens"),
        "rule id missing from output: {output}"
    );
}

#[test]
fn warning_level_does_not_fail() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("warn.yaml");
    fs::write(&file, "---\n-  item\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  hyphens:\n    level: warning\n",
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
fn rule_ignore_skips_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "---\n-  item\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  hyphens:\n    ignore:\n      - ignored.yaml\n",
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
fn dash_on_own_line_flags_inline_mapping_via_toml() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("inline.yaml");
    fs::write(&file, "items:\n  - name: web\n    port: 80\n").unwrap();
    let config = dir.path().join("config.toml");
    fs::write(&config, "[rules.hyphens]\ndash-on-own-line = true\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("block mapping should start on a new line after the hyphen"),
        "missing dash-on-own-line message: {output}"
    );
    // Bare `line:col` (the mapping key) and bare rule id appear in both output formats.
    assert!(output.contains("2:5"), "expected span at the key: {output}");
    assert!(output.contains("hyphens"), "rule id missing: {output}");
}

#[test]
fn dash_on_own_line_accepts_dash_alone_via_toml() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("own-line.yaml");
    // Dash alone with the mapping body indented below is the layout the option wants.
    fs::write(&file, "items:\n  -\n    name: web\n    port: 80\n").unwrap();
    let config = dir.path().join("config.toml");
    fs::write(&config, "[rules.hyphens]\ndash-on-own-line = true\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));

    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
}

#[test]
fn dash_on_own_line_rejected_in_yaml_config() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "items:\n  - name: web\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  hyphens:\n    dash-on-own-line: true\n")
        .arg(&file));

    assert_eq!(
        code, 2,
        "ryl-only option must be rejected in YAML config: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("hyphens"),
        "expected config rejection mentioning hyphens: {output}"
    );
}

#[test]
fn custom_max_allows_extra_spacing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("custom.yaml");
    fs::write(&file, "---\n-   item\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  hyphens:\n    max-spaces-after: 3\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "custom max should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}
