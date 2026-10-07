use std::fs;
use std::process::{Command, Stdio};

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

const DEFAULT: &str = "extends: default";

fn check_fix(config: &str, input: &str) -> String {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, input).unwrap();
    let (code, stdout, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["check", "--fix", "-d", config])
        .arg(&file));
    assert!(
        code <= 1,
        "unexpected failure: stdout={stdout} stderr={stderr}"
    );
    fs::read_to_string(&file).unwrap()
}

fn check(config: &str, input: &str) -> String {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, input).unwrap();
    let (code, stdout, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["check", "-f", "parsable", "-d", config])
        .arg(&file));
    assert_eq!(code, 0, "warnings only: stderr={stderr}");
    format!("{stdout}{stderr}")
}

#[test]
fn fix_keeps_magic_first_line_first() {
    let cases = [
        (
            "#!/usr/bin/env yq\na: 1\n",
            "#!/usr/bin/env yq\n---\na: 1\n",
        ),
        (
            "#cloud-config\npackages: [nginx]\n",
            "#cloud-config\n---\npackages: [nginx]\n",
        ),
        (
            "#cloud-config-archive\na: 1\n",
            "#cloud-config-archive\n---\na: 1\n",
        ),
        ("##!x\na: 1\n", "##!x\n---\na: 1\n"),
        (
            "\u{feff}#cloud-config\na: 1\n",
            "\u{feff}#cloud-config\n---\na: 1\n",
        ),
        (
            "#cloud-config\n#x\na: 1\n",
            "#cloud-config\n---\n# x\na: 1\n",
        ),
        (
            "#cloud-config\n%YAML 1.2\n---\na: 1\n",
            "#cloud-config\n%YAML 1.2\n---\na: 1\n",
        ),
        (
            "# -*- coding: utf-8 -*-\na: 1\n",
            "---\n# -*- coding: utf-8 -*-\na: 1\n",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(check_fix(DEFAULT, input), expected, "input: {input:?}");
    }
}

#[test]
fn fix_inserts_marker_after_crlf_shebang_and_before_unterminated_content() {
    let dos = "extends: default\nrules:\n  new-lines: {type: dos}\n";
    assert_eq!(
        check_fix(dos, "#!/x\r\na: 1\r\n"),
        "#!/x\r\n---\r\na: 1\r\n"
    );
    let no_eof = "extends: default\nrules:\n  new-line-at-end-of-file: disable\n";
    assert_eq!(check_fix(no_eof, "a: 1"), "---\na: 1");
}

#[test]
fn fix_spaces_cloud_config_when_shebangs_are_not_ignored() {
    let config = "extends: default\nrules:\n  comments: {ignore-shebangs: false}\n";
    assert_eq!(
        check_fix(config, "#cloud-config\na: 1\n"),
        "---\n# cloud-config\na: 1\n"
    );
}

#[test]
fn fix_spaces_line_one_shebang_after_content() {
    let config = "extends: default\nrules:\n  document-start: disable\n";
    assert_eq!(check_fix(config, "a: 1  #!x\n"), "a: 1  # !x\n");
    assert!(
        check(config, "a: 1  #!x\n").contains(":1:8: [warning] missing starting space"),
        "comments diagnostic expected"
    );
}

#[test]
fn check_reports_only_document_start_at_line_two() {
    for input in ["#!/usr/bin/env yq\na: 1\n", "#cloud-config\na: 1\n"] {
        let output = check(DEFAULT, input);
        assert_eq!(
            output.lines().count(),
            1,
            "one diagnostic expected: {output}"
        );
        assert!(
            output.contains(":2:1: [warning] missing document start"),
            "document-start at 2:1 expected: {output}"
        );
    }
}

#[test]
fn format_keeps_cloud_config_first() {
    let dir = tempdir().unwrap();
    let mut child = ryl(dir.path())
        .current_dir(dir.path())
        .args(["format", "--stdin-filename", "u.yaml", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(
        &mut child.stdin.take().unwrap(),
        b"#cloud-config\npackages: [nginx]\n",
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "#cloud-config\n---\npackages: [nginx]\n"
    );
}
