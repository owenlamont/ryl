use std::fs;

use tempfile::tempdir;

use crate::common;
use common::cli::{command_output, run, ryl};

const TARGET_1_2: &str =
    "target-version = \"1.2\"\n[lint.rules]\nyaml-directive = \"enable\"\n";

fn ryl_with(config: &str, content: &str, args: &[&str]) -> (i32, String, String) {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, content).unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).args(args).arg(&file));
    let output = command_output(&stdout, &stderr).to_string();
    (code, output, fs::read_to_string(&file).unwrap())
}

#[test]
fn reports_each_document_without_a_directive() {
    let (code, output, _) = ryl_with(
        TARGET_1_2,
        "#!/usr/bin/env tool\na: 1\n--- # two\nb: 2\n...\n%YAML 1.2\n---\nc: 3\n",
        &["check"],
    );
    assert_eq!(code, 1, "{output}");
    assert!(output.contains("2:1") && output.contains("3:1"), "{output}");
    assert!(
        !output.contains("7:1"),
        "a declared document passes: {output}"
    );
    assert!(
        output.contains("missing version directive")
            && output.contains("yaml-directive"),
        "{output}"
    );
    let (code, output, _) = ryl_with(TARGET_1_2, "# only a comment\n", &["check"]);
    assert_eq!(
        code, 0,
        "a stream with no document has nothing to declare: {output}"
    );
}

#[test]
fn fix_stamps_yaml_1_2_and_ends_an_open_document_first() {
    let (code, output, fixed) = ryl_with(
        TARGET_1_2,
        "#cloud-config\na: |+\n  x\n\n---\nb: 2\n...\nc: 3\n",
        &["check", "--fix"],
    );
    assert_eq!(code, 0, "{output}");
    assert_eq!(
        fixed,
        "#cloud-config\n%YAML 1.2\n---\na: |+\n  x\n\n...\n%YAML 1.2\n---\nb: 2\n...\n\
         %YAML 1.2\n---\nc: 3\n"
    );
}

#[test]
fn fix_is_offered_only_under_target_1_2() {
    let input = "a: 1\n";
    let config = "[lint.rules]\nyaml-directive = \"enable\"\n";
    let (code, output, fixed) = ryl_with(config, input, &["check", "--fix"]);
    assert_eq!(code, 1, "{output}");
    assert_eq!(
        fixed, input,
        "stamping 1.2 under target 1.1 drops 1.1 quoting"
    );
}

#[test]
fn fix_declines_a_needed_end_marker_that_document_end_forbids() {
    let config = format!("{TARGET_1_2}document-end = {{ present = false }}\n");
    let (code, output, fixed) =
        ryl_with(&config, "a: 1\n---\nb: 2\n", &["check", "--fix"]);
    assert_eq!(code, 1, "{output}");
    assert_eq!(fixed, "%YAML 1.2\n---\na: 1\n---\nb: 2\n");
    assert!(
        output.contains("4:1") && output.contains("yaml-directive"),
        "{output}"
    );
}

#[test]
fn document_start_present_false_rejects_yaml_directive_by_name() {
    for (config, args) in [
        (
            format!("{TARGET_1_2}document-start = {{ present = false }}\n"),
            &["check"][..],
        ),
        (
            "[lint.rules]\ndocument-start = { present = false }\n".to_string(),
            &["check", "--enable", "yaml-directive,document-start"][..],
        ),
        (
            "[lint.rules]\ndocument-start = { present = false }\n".to_string(),
            &["check", "--enable", "ALL", "--enable", "yaml-directive"][..],
        ),
        (
            "[lint.rules]\nALL = \"enable\"\nyaml-directive = \"enable\"\n\
             document-start = { present = false }\n"
                .to_string(),
            &["check"][..],
        ),
    ] {
        let (code, output, _) = ryl_with(&config, "a: 1\n", args);
        assert_eq!(code, 2, "{args:?}: {output}");
        assert!(
            output.contains("yaml-directive needs the document start"),
            "{args:?}: {output}"
        );
    }
}

#[test]
fn enable_all_ignores_yaml_directive_beside_document_start_present_false() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[lint.rules]\ndocument-start = { present = false }\n",
    )
    .unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    let check = |extra: &[&str]| {
        run(ryl(dir.path())
            .args(["check", "--enable", "ALL"])
            .args(extra)
            .arg(&file))
    };
    let (code, stdout, stderr) = check(&["--disable", "yaml-directive"]);
    assert_eq!(
        check(&[]),
        (
            code,
            stdout,
            format!(
                "warning: `yaml-directive` and `document-start` with `present = false` \
                 are incompatible. Ignoring `yaml-directive`.\n{stderr}"
            )
        ),
        "the warning alone changes nothing else"
    );
}

#[test]
fn config_all_ignores_yaml_directive_beside_document_start_present_false() {
    let config = "[lint.rules]\nALL = \"enable\"\ndocument-start = { present = false }\n\
                  document-end = \"disable\"\n";
    let (code, output, _) = ryl_with(config, "a: 1\n", &["check"]);
    assert_eq!(
        (code, output.as_str()),
        (
            0,
            "warning: `yaml-directive` and `document-start` with `present = false` are \
             incompatible. Ignoring `yaml-directive`.\n"
        )
    );
}
