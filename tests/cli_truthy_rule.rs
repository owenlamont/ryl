use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

#[test]
fn truthy_rule_reports_plain_truthy_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("values.yaml");
    fs::write(&file, "foo: True\nbar: yes\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  truthy: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 1,
        "expected lint failure: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("truthy value should be one of [false, true]"),
        "missing truthy message: {output}"
    );
    assert!(output.contains("truthy"), "rule label missing: {output}");
    assert!(
        output.contains("1:6"),
        "expected position for value 'True': {output}"
    );
    assert!(
        output.contains("2:6"),
        "expected position for value 'yes': {output}"
    );
}

#[test]
fn truthy_rule_respects_check_keys_false() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("keys.yaml");
    fs::write(&file, "True: yes\nvalue: True\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  truthy:\n    allowed-values: []\n    check-keys: false\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 1,
        "expected lint failure: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("2:8"),
        "value position should be reported when keys are skipped: {output}"
    );
    assert!(
        !output.contains("1:1"),
        "keys should be ignored when disabled: {output}"
    );
}

fn fix_with_config(toml: &str, input: &str) -> (i32, String, String) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, input).unwrap();
    fs::write(dir.path().join(".ryl.toml"), toml).unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));
    (
        code,
        fs::read_to_string(&file).unwrap(),
        format!("{stdout}{stderr}"),
    )
}

#[test]
fn truthy_fix_recases_booleans_but_not_yes_no_or_disabled_lines() {
    let (code, fixed, output) = fix_with_config(
        "[rules.truthy]\n",
        "enabled: TRUE\nvisible: False\nlabel: yes\nmode: off\nkeep: True  # ryl disable-line rule:truthy\n",
    );
    assert_eq!(code, 1, "yes/off remain: {output}");
    assert!(
        output.contains("Found 4 problems (2 fixed, 2 remaining)."),
        "{output}"
    );
    assert_eq!(
        fixed,
        "enabled: true\nvisible: false\nlabel: yes\nmode: off\nkeep: True  # ryl disable-line rule:truthy\n"
    );
}

#[test]
fn truthy_fix_honours_fixable_and_unfixable() {
    let rules = "[rules.truthy]\n[rules.trailing-spaces]\n[fix]\n";
    for (fix_table, expected) in [
        ("unfixable = ['truthy']\n", "a: True\n"),
        ("fixable = ['truthy']\n", "a: true \n"),
    ] {
        let (_, fixed, output) =
            fix_with_config(&format!("{rules}{fix_table}"), "a: True \n");
        assert_eq!(fixed, expected, "{fix_table}: {output}");
    }
}

#[test]
fn truthy_fix_on_keys_surfaces_an_existing_duplicate() {
    let (code, fixed, output) = fix_with_config(
        "[rules.truthy]\n[rules.key-duplicates]\n",
        "True: 1\ntrue: 2\n",
    );
    assert_eq!(code, 1, "{output}");
    assert_eq!(fixed, "true: 1\ntrue: 2\n");
    assert!(
        output.contains("2:1") && output.contains("duplication of key \"true\""),
        "{output}"
    );
}
