use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

/// `unicode-line-breaks` is a ryl-only rule, so it is configured through TOML
/// rather than the yamllint-compatible YAML config that `-d` carries.
fn lint_with_toml_config(content: &str, config: &str) -> (i32, String) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, content).unwrap();
    let config_path = dir.path().join(".ryl.toml");
    fs::write(&config_path, config).unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config_path).arg(&file));
    (code, command_output(&stdout, &stderr).to_string())
}

#[test]
fn flags_nel_ls_ps_across_contexts_with_char_based_columns() {
    // LS in a double-quoted scalar (1), NEL in a plain scalar (2), PS in a
    // comment (3), and LS after a multibyte key (4): proving the rule fires
    // anywhere and that columns count characters, not bytes (col 8 past `café`).
    let (code, output) = lint_with_toml_config(
        "key: \"a\u{2028}b\"\nplain: x\u{85}y\n# c\u{2029}d\ncafé: \"\u{2028}\"\n",
        "[rules]\nunicode-line-breaks = \"enable\"\n",
    );
    assert_eq!(code, 1, "raw line-break characters should fail: {output}");
    assert!(
        output.contains("1:8")
            && output.contains("line separator")
            && output.contains("\\L"),
        "LS in a quoted scalar at 1:8 with its \\L escape: {output}"
    );
    assert!(
        output.contains("2:9")
            && output.contains("next line")
            && output.contains("\\N"),
        "NEL in a plain scalar at 2:9 with its \\N escape: {output}"
    );
    assert!(
        output.contains("3:4")
            && output.contains("paragraph separator")
            && output.contains("\\P"),
        "PS in a comment at 3:4 with its \\P escape: {output}"
    );
    assert!(
        output.contains("4:8"),
        "char-based column past the multibyte key café: {output}"
    );
    assert!(
        output.contains("unicode-line-breaks"),
        "rule id missing: {output}"
    );
}

#[test]
fn flags_chars_adjacent_to_cr_and_crlf_breaks() {
    // The rule must keep NEL/LS/PS on the line they sit on while still advancing the
    // counter across `\r\n` and bare `\r`: the chars are not YAML 1.2 breaks but the
    // CRs are. All-comment lines keep the doc valid YAML so the rule spans survive.
    // LS before a CRLF (1:4), then NEL before a bare CR (2:4); the final `\n` line is
    // clean.
    let (code, output) = lint_with_toml_config(
        "# a\u{2028}\r\n# b\u{85}\r# c\n",
        "[rules]\nunicode-line-breaks = \"enable\"\n",
    );
    assert_eq!(code, 1, "raw breaks adjacent to CRs should fail: {output}");
    assert!(
        output.contains("1:4") && output.contains("line separator"),
        "LS before a CRLF stays at 1:4: {output}"
    );
    assert!(
        output.contains("2:4") && output.contains("next line"),
        "NEL before a bare CR stays at 2:4 (the CR advanced the line): {output}"
    );
}

#[test]
fn rule_does_not_fire_when_not_enabled() {
    let (code, output) =
        lint_with_toml_config("a: \"x\u{2028}y\"\n", "[rules]\ntruthy = \"enable\"\n");
    assert_eq!(code, 0, "rule is off unless enabled: {output}");
    assert!(
        !output.contains("unicode-line-breaks"),
        "rule must not run unless enabled: {output}"
    );
}

#[test]
fn rule_is_rejected_in_yaml_config() {
    // ryl-only: yamllint-compatible YAML config (here via `-d`) must reject it
    // rather than silently linting or clashing with a future yamllint rule.
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "a: \"x\u{2028}y\"\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules: {unicode-line-breaks: enable}")
        .arg(&file));
    assert_eq!(
        code, 2,
        "a ryl-only rule in YAML config is a usage error: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("unicode-line-breaks"),
        "error should name the rule: {output}"
    );
    assert!(
        output.to_lowercase().contains("toml"),
        "error should point to TOML config: {output}"
    );
}

#[test]
fn per_file_ignores_accept_the_rule_name() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "a: \"x\u{2028}y\"\n").unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        format!(
            "[rules]\nunicode-line-breaks = \"enable\"\n[per-file-ignores]\n'{}' = ['unicode-line-breaks']\n",
            file.display()
        ),
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "per-file-ignores should suppress the rule: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}
