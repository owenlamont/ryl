use std::fs;
use std::process::Command;

use tempfile::tempdir;

use crate::common;
use common::cli::{command_output, run, ryl};

fn run_with_config(config: &str, content: &str, extra: &[&str]) -> (i32, String) {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yaml");
    fs::write(&cfg, config).unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, content).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut command = Command::new(exe);
    command.arg("-c").arg(&cfg).args(extra).arg(&file);
    let (code, stdout, stderr) = run(&mut command);
    (code, command_output(&stdout, &stderr).to_string())
}

const TRUTHY: &str = "rules:\n  document-start: disable\n  truthy: enable\n";

#[test]
fn rejects_higher_major_version_directive() {
    let (code, output) = run_with_config(TRUTHY, "%YAML 2.0\n---\nkey: value\n", &[]);
    assert_eq!(code, 1, "higher major version must fail: {output}");
    assert!(
        output.contains("found incompatible YAML document"),
        "expected the incompatible-version error: {output}"
    );
    assert!(
        output.contains("1:1"),
        "expected directive position: {output}"
    );
}

#[test]
fn refuses_to_fix_a_higher_major_version_directive() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yaml");
    fs::write(
        &cfg,
        "rules:\n  quoted-strings:\n    required: only-when-needed\n",
    )
    .unwrap();
    let file = dir.path().join("input.yaml");
    let original = "%YAML 2.0\n---\nkey: 'no'\n";
    fs::write(&file, original).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&cfg)
        .arg("--fix")
        .arg(&file));
    assert_eq!(code, 1, "fix must report the unfixable file");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("skipped by --fix"),
        "missing skip notice: {output}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original,
        "a 2.0 document must not be mutated"
    );
}

#[test]
fn warns_on_higher_minor_version_and_resolves_it_as_1_2() {
    // `%YAML 1.3` is processed as 1.2, so `yes` is a plain string: the run warns about
    // the version but `truthy` must not flag it (exit stays 0).
    let (code, output) = run_with_config(TRUTHY, "%YAML 1.3\n---\non: yes\n", &[]);
    assert_eq!(
        code, 0,
        "a higher minor version is a warning, not a failure: {output}"
    );
    assert!(
        output.contains("newer than 1.2"),
        "expected the higher-minor warning: {output}"
    );
    assert!(
        !output.contains("truthy value should be one of"),
        "1.3 is processed as 1.2, so truthy must not flag `yes`: {output}"
    );
}

#[test]
fn higher_minor_warning_fails_under_strict() {
    let (code, _) =
        run_with_config(TRUTHY, "%YAML 1.3\n---\nkey: value\n", &["--strict"]);
    assert_eq!(code, 2, "--strict promotes the warning to a non-zero exit");
}

#[test]
fn rejects_duplicate_version_directives() {
    let (code, output) =
        run_with_config(TRUTHY, "%YAML 1.1\n%YAML 1.2\n---\nkey: value\n", &[]);
    assert_eq!(code, 1, "duplicate directives must fail: {output}");
    assert!(
        output.contains("duplicate version directive"),
        "expected the duplicate-directive error: {output}"
    );
}

#[test]
fn directive_like_block_scalar_content_is_not_a_directive() {
    // The `%YAML 2.0` is block-scalar content, not a directive (a directive is
    // line-initial), so the file is valid and must not be rejected.
    let (code, output) = run_with_config(
        TRUTHY,
        "description: |\n  %YAML 2.0 is the next major version\nkey: value\n",
        &[],
    );
    assert_eq!(
        code, 0,
        "block-scalar `%YAML` content must not be rejected: {output}"
    );
}

#[test]
fn a_directive_does_not_leak_into_a_later_document() {
    // The `%YAML 1.2` is block-scalar content in document 1, so document 2 declares no
    // version and keeps the quotes a YAML 1.1 reader needs.
    let config = "rules:\n  quoted-strings:\n    required: only-when-needed\n";
    let (code, output) =
        run_with_config(config, "first: |\n  %YAML 1.2\n---\nkey: 'no'\n", &[]);
    assert_eq!(code, 0, "document 2 must keep its 1.1 quotes: {output}");
}

#[test]
fn rules_agree_under_explicit_yaml_1_1() {
    let config = "rules:\n  document-start: disable\n  truthy: enable\n  \
                  quoted-strings:\n    required: only-when-needed\n";
    let (code, output) =
        run_with_config(config, "%YAML 1.1\n---\nplain: no\nquoted: 'no'\n", &[]);
    assert_eq!(code, 1, "the plain truthy value must still flag: {output}");
    assert!(
        output.contains("truthy value should be one of"),
        "missing truthy: {output}"
    );
    assert!(
        !output.contains("redundantly quoted"),
        "quoted-strings must keep the 1.1-ambiguous quote: {output}"
    );
}

fn format_with_toml(toml: &str, content: &str) -> String {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), toml).unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, content).unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).arg("format").arg(&file));
    assert_eq!(code, 0, "{}", command_output(&stdout, &stderr));
    fs::read_to_string(&file).unwrap()
}

#[test]
fn target_1_2_or_an_explicit_1_2_drops_the_quotes_only_yaml_1_1_needs() {
    let quoted = "a: 'no'\nb: '1_000'\nc: '2026-10-10'\n";
    let plain = "a: no\nb: 1_000\nc: 2026-10-10\n";
    assert_eq!(format_with_toml("[format]\n", quoted), quoted);
    assert_eq!(
        format_with_toml("target-version = \"1.2\"\n[format]\n", quoted),
        plain
    );
    let declared = format!("%YAML 1.2\n---\n{quoted}");
    assert_eq!(
        format_with_toml("[format]\n", &declared),
        format!("%YAML 1.2\n---\n{plain}")
    );
}

#[test]
fn truthy_reads_yaml_1_2_words_under_target_1_2() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "target-version = \"1.2\"\n[lint.rules]\ntruthy = \"enable\"\n",
    )
    .unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "a: yes\nb: True\n...\n%YAML 1.1\n---\nc: yes\n").unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).arg("check").arg(&file));
    let output = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "{output}");
    assert!(!output.contains("1:4"), "`yes` is a 1.2 string: {output}");
    assert!(output.contains("2:4"), "`True` is a 1.2 boolean: {output}");
    assert!(output.contains("6:4"), "`%YAML 1.1` reads `yes`: {output}");
}
