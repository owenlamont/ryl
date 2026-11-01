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
    let input = "%YAML 1.1\n---\na: yes\n";
    let (code, output, fixed) = ryl_with(config, input, &["check", "--fix"]);
    assert_eq!(
        (code, output.as_str(), fixed.as_str()),
        (0, "", input),
        "target 1.1 neither reports nor migrates a 1.1 document"
    );
}

#[test]
fn fix_migrates_each_yaml_1_1_scalar_to_a_1_2_spelling_of_its_value() {
    let (code, output, fixed) = ryl_with(
        TARGET_1_2,
        "%YAML 1.1 # keep\n---\nyes: on\nb: 0777\nc: 22:22\nd: 1:30.5\ne: 1_000\n\
         f: 0b1010\ng: !!bool yes\nh: -0777\n\
         i: [~, .inf, .NaN, <<, 0x1F, -0x1_f, 0x1_f, 6_0:00:00.5, 1.5e+3, 1_0.5_, &x 1:0:0]\n\
         m: [-1:30.5, No]\n\
         j: [\"yes\", ! word, !!str off, ._5]\nk: !!int 0_777\nl: !!float 1_0.5\n",
        &["check", "--fix"],
    );
    assert_eq!(code, 0, "{output}");
    assert_eq!(
        fixed,
        "%YAML 1.2 # keep\n---\ntrue: true\nb: 0o777\nc: 1342\nd: 90.5\ne: 1000\n\
         f: 10\ng: !!bool true\nh: -511\n\
         i: [~, .inf, .NaN, <<, 0x1F, -31, 0x1f, 216000.5, 1.5e+3, 10.5, &x 3600]\n\
         m: [-90.5, false]\n\
         j: [\"yes\", ! word, !!str off, ._5]\nk: !!int 0o777\nl: !!float 10.5\n"
    );
}

#[test]
fn migration_declines_a_document_holding_a_value_1_2_cannot_carry() {
    let declining = [
        "%YAML 1.1\n---\na: yes\nb: 08\n",
        "%YAML 1.1\n---\na: yes\nb: 0b_\n",
        "%YAML 1.1\n---\na: yes\nb: !!int yes\n",
        "%YAML 1.1\n---\na: yes\nb: !!bool 'yes'\n",
        "%YAML 1.1\n---\na: yes\nb: ! 0777\n",
        "%YAML 1.0\n---\na: yes\nb: 2026-10-10\n",
    ]
    .join("...\n");
    let input = format!("%YAML 1.1\n---\na: yes\n...\n{declining}");
    let (code, output, fixed) = ryl_with(TARGET_1_2, &input, &["check", "--fix"]);
    assert_eq!(fixed, format!("%YAML 1.2\n---\na: true\n...\n{declining}"));
    assert_eq!(code, 1, "{output}");
    let older: Vec<&str> = output
        .lines()
        .filter(|line| line.contains("is older than target-version \"1.2\""))
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(
        older,
        ["5:1", "10:1", "15:1", "20:1", "25:1", "30:1"],
        "{output}"
    );
    assert!(
        output.contains("version directive \"%YAML 1.0\""),
        "{output}"
    );
    for line_break in ['\u{85}', '\u{2028}', '\u{2029}'] {
        let input = format!("%YAML 1.1\n---\na: \"abc{line_break}  def\"\nb: yes\n");
        let (_, output, fixed) = ryl_with(TARGET_1_2, &input, &["check", "--fix"]);
        assert_eq!(fixed, input, "1.1 folds the break, 1.2 keeps it: {output}");
    }
}

#[test]
fn migration_never_half_applies_around_a_disabled_line() {
    for (input, fixed) in [
        (
            "%YAML 1.1\n---\na: yes\nb: 0777 # ryl disable-line rule:yaml-directive\n",
            None,
        ),
        (
            "%YAML 1.1 # ryl disable-line rule:yaml-directive\n---\na: 1\n",
            None,
        ),
        (
            "%YAML 1.1 # ryl disable-line rule:yaml-directive\n---\na: yes\n",
            Some("%YAML 1.1 # ryl disable-line rule:yaml-directive\n---\na: true\n"),
        ),
    ] {
        let (_, output, after) = ryl_with(TARGET_1_2, input, &["check", "--fix"]);
        assert_eq!(after, fixed.unwrap_or(input), "{output}");
    }
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
