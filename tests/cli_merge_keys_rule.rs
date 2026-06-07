use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn command_output<'a>(stdout: &'a str, stderr: &'a str) -> &'a str {
    if stderr.is_empty() { stdout } else { stderr }
}

/// `merge-keys` is a ryl-only rule, so it is configured through TOML rather than
/// the yamllint-compatible YAML config that `-d` carries.
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

const ENABLE: &str = "[rules]\nmerge-keys = \"enable\"\n";

#[test]
fn flags_merge_key_for_alias_inline_and_sequence_values() {
    // A plain `<<` merges whether its value is an alias, an inline mapping, or a
    // sequence of either — the key itself is flagged in every case.
    let (code, output) = lint_with_toml_config(
        "base: &b {x: 1}\nalias:\n  <<: *b\ninline:\n  <<: {y: 2}\nseq:\n  <<: [*b]\n",
        ENABLE,
    );
    assert_eq!(code, 1, "merge keys should fail: {output}");
    for pos in ["3:3", "5:3", "7:3"] {
        assert!(
            output.contains(pos),
            "expected a merge-key diagnostic at {pos}: {output}"
        );
    }
    assert!(
        output.contains("forbidden merge key") && output.contains("merge-keys"),
        "message text and bare rule id expected: {output}"
    );
}

#[test]
fn flags_flow_merge_key_with_char_based_column() {
    // `<<` after a multibyte key in a flow mapping: column 17 counts characters,
    // not bytes (`é` in `café` is two bytes, which would push the byte offset to
    // 18).
    let (code, output) =
        lint_with_toml_config("base: &b {x: 1}\nflow: {café: 1, <<: *b}\n", ENABLE);
    assert_eq!(code, 1, "flow merge key should fail: {output}");
    assert!(
        output.contains("2:17"),
        "char-based column past the multibyte key café: {output}"
    );
}

#[test]
fn does_not_flag_quoted_merge_key() {
    // A quoted `"<<"` is a plain string key that never merges (PyYAML/ruamel),
    // so it is the portable form and must not be flagged.
    let (code, output) =
        lint_with_toml_config("base: &b {x: 1}\nchild:\n  \"<<\": *b\n", ENABLE);
    assert_eq!(code, 0, "a quoted '<<' is not a merge key: {output}");
    assert!(
        !output.contains("merge-keys"),
        "quoted '<<' must not be flagged: {output}"
    );
}

#[test]
fn flags_explicitly_tagged_merge_key_regardless_of_text() {
    // An explicit `!!merge` tag performs a merge regardless of the scalar's text
    // (verified in PyYAML and ruamel), so `!!merge foo` is a merge directive even
    // though its text is not `<<`; the diagnostic names the actual key.
    let (code, output) =
        lint_with_toml_config("base: &b {x: 1}\nchild:\n  !!merge foo: *b\n", ENABLE);
    assert_eq!(code, 1, "an explicit !!merge key should fail: {output}");
    assert!(
        output.contains("3:11")
            && output.contains("forbidden merge key \"foo\"")
            && output.contains("merge-keys"),
        "tagged merge flagged with the actual key text: {output}"
    );
}

#[test]
fn flags_verbatim_merge_tag() {
    // A verbatim-spelled core merge tag merges identically (verified in PyYAML and
    // ruamel) but resolves to an empty handle, so it is matched by the tag's full
    // URI rather than granit's handle-only `is_yaml_core_schema`.
    let (code, output) = lint_with_toml_config(
        "base: &b {x: 1}\n!<tag:yaml.org,2002:merge> foo: *b\n",
        ENABLE,
    );
    assert_eq!(code, 1, "a verbatim merge tag should fail: {output}");
    assert!(
        output.contains("2:28")
            && output.contains("forbidden merge key \"foo\"")
            && output.contains("merge-keys"),
        "verbatim merge tag flagged with the actual key text: {output}"
    );
}

#[test]
fn rule_does_not_fire_when_not_enabled() {
    let (code, output) = lint_with_toml_config(
        "base: &b {x: 1}\nchild:\n  <<: *b\n",
        "[rules]\ntruthy = \"enable\"\n",
    );
    assert_eq!(code, 0, "rule is off unless enabled: {output}");
    assert!(
        !output.contains("merge-keys"),
        "rule must not run unless enabled: {output}"
    );
}

#[test]
fn rule_is_rejected_in_yaml_config() {
    // ryl-only: yamllint-compatible YAML config (here via `-d`) must reject it
    // rather than silently linting or clashing with a future yamllint rule.
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "base: &b {x: 1}\nchild:\n  <<: *b\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules: {merge-keys: enable}")
        .arg(&file));
    assert_eq!(
        code, 2,
        "a ryl-only rule in YAML config is a usage error: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("merge-keys"),
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
    fs::write(&file, "base: &b {x: 1}\nchild:\n  <<: *b\n").unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        format!(
            "[rules]\nmerge-keys = \"enable\"\n[per-file-ignores]\n'{}' = ['merge-keys']\n",
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
