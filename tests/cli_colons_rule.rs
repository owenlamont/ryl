use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

#[test]
fn colons_reports_spacing_errors() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "---\nkey :  value\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  colons: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("too many spaces before colon"),
        "missing before-colon message: {output}"
    );
    assert!(
        output.contains("too many spaces after colon"),
        "missing after-colon message: {output}"
    );
    assert!(output.contains("colons"), "rule id missing: {output}");
}

#[test]
fn colons_allows_required_space_for_alias_mapping_key() {
    // `*foo : bar` needs the space (without it `:` joins the alias
    // name), so it must not be flagged; an extra space still is.
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  colons: enable\n",
    )
    .unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");

    let ok = dir.path().join("alias-ok.yaml");
    fs::write(&ok, "---\na: &foo 42\nm:\n  *foo : bar\n").unwrap();
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&ok));
    assert_eq!(
        code, 0,
        "alias key with the required space must be clean: stdout={stdout} stderr={stderr}"
    );

    let bad = dir.path().join("alias-bad.yaml");
    fs::write(&bad, "---\na: &foo 42\nm:\n  *foo  : bar\n").unwrap();
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&bad));
    assert_eq!(
        code, 1,
        "an extra space before the alias-key colon must fail: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(output.contains("4:8"), "expected line:col 4:8: {output}");
    assert!(
        output.contains("too many spaces before colon"),
        "missing before-colon message: {output}"
    );
    assert!(output.contains("colons"), "rule id missing: {output}");
}

#[test]
fn colons_reports_explicit_key_spacing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("explicit.yaml");
    fs::write(&file, "---\n?  key\n: value\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  colons: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("too many spaces after question mark"),
        "missing question mark message: {output}"
    );
}

#[test]
fn colons_reports_and_fixes_explicit_keys_after_unicode() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("explicit-unicode.yaml");
    fs::write(&file, "café: one\n?  key\n: value\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .args(["check", "--no-cache", "-d", "[lint.rules.colons]"])
        .arg(&file));
    assert_eq!(code, 1, "stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(output.contains("2:3"), "wrong character column: {output}");
    assert!(
        output.contains("too many spaces after question mark"),
        "missing question mark diagnostic: {output}"
    );
    let (code, stdout, stderr) = run(Command::new(exe)
        .args(["check", "--fix", "--no-cache", "-d", "[lint.rules.colons]"])
        .arg(&file));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "café: one\n? key\n: value\n"
    );
}

#[test]
fn colons_reports_character_columns_with_unicode_and_mixed_breaks() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("unicode.yaml");
    fs::write(&file, "café:  one\r\nx:  two\r").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .args(["check", "-d", "rules: {colons: enable}"])
        .arg(&file));
    assert_eq!(code, 1, "stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(output.contains("1:7"), "Unicode column changed: {output}");
    assert!(output.contains("2:4"), "bare-CR line changed: {output}");
}

#[test]
fn fix_with_a_huge_tolerance_leaves_clean_input_alone() {
    let dir = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    for (rule, content) in [("colons", "a: 1\n"), ("hyphens", "- x\n")] {
        let file = dir.path().join(format!("{rule}.yaml"));
        fs::write(&file, content).unwrap();
        let config = format!("rules: {{{rule}: {{max-spaces-after: {}}}}}", i64::MAX);
        let (code, _, stderr) = run(Command::new(exe)
            .args(["check", "--fix", "-d", &config])
            .arg(&file));
        assert_eq!(code, 0, "{rule}: {stderr}");
        assert_eq!(fs::read_to_string(&file).unwrap(), content, "{rule}");
    }
}

#[test]
fn fix_leaves_adjacent_flow_values_that_max_spaces_allows() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("adjacent.yaml");
    fs::write(&file, "- {\"café\":value}\n").unwrap();
    let (code, _, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["check", "--fix", "-d", "rules:\n  colons: enable\n"])
        .arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(fs::read_to_string(&file).unwrap(), "- {\"café\":value}\n");
}
