use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::tempdir;

use ryl::rules::document_start::{self, Config};

mod common;
use common::cli::{run, ryl};

#[test]
fn fix_adds_a_marker_to_every_implicit_document() {
    let cases = [
        ("a: 1\n---\nb: 2\n", "---\na: 1\n---\nb: 2\n"),
        ("a: 1\n...\nb: 2\n", "---\na: 1\n...\n---\nb: 2\n"),
        (
            "a: 1\n...\nb: 2\n...\nc: 3\n",
            "---\na: 1\n...\n---\nb: 2\n...\n---\nc: 3\n",
        ),
        (
            "---\na: 1\n...\n# c\n\nb: 2\n",
            "---\na: 1\n...\n---\n# c\n\nb: 2\n",
        ),
        (
            "a: 1\n... # end\nb: 2\n",
            "---\na: 1\n... # end\n---\nb: 2\n",
        ),
        ("...\na: 1\n", "...\n---\na: 1\n"),
        ("# c\n...\n...\na: 1\n", "# c\n...\n...\n---\na: 1\n"),
        ("x: |\n  ---\n  ...\n", "---\nx: |\n  ---\n  ...\n"),
        ("#!x\na: 1\n---\nb: 2\n", "#!x\n---\na: 1\n---\nb: 2\n"),
        (
            "#cloud-config\na: 1\n...\nb: 2\n",
            "#cloud-config\n---\na: 1\n...\n---\nb: 2\n",
        ),
        (
            "\u{feff}a: 1\n---\nb: 2\n",
            "\u{feff}---\na: 1\n---\nb: 2\n",
        ),
        (
            "a: 1\r\n...\r\nb: 2\r\n",
            "---\r\na: 1\r\n...\r\n---\r\nb: 2\r\n",
        ),
        ("a: 1\r...\rb: 2\r", "---\ra: 1\r...\r---\rb: 2\r"),
        (
            "%YAML 1.2\n---\na: 1\n...\nb: 2\n",
            "%YAML 1.2\n---\na: 1\n...\n---\nb: 2\n",
        ),
        (
            "a: 1\n...\n\u{feff}\nb: 2\n",
            "---\na: 1\n...\n\u{feff}---\n\nb: 2\n",
        ),
        (
            "a: 1\n...\n# c\n\u{feff}b: 2\n",
            "---\na: 1\n...\n# c\n\u{feff}---\nb: 2\n",
        ),
        (
            "a: 1\n...\n\u{feff}# c\nb: 2\n",
            "---\na: 1\n...\n\u{feff}---\n# c\nb: 2\n",
        ),
        ("\u{feff}#!x\na: 1\n", "\u{feff}#!x\n---\na: 1\n"),
        ("\u{feff}# c\na: 1\n", "\u{feff}---\n# c\na: 1\n"),
    ];
    let cfg = Config::new(true);
    for (input, expected) in cases {
        assert_eq!(
            document_start::fix(input, &cfg).as_deref(),
            Some(expected),
            "input: {input:?}"
        );
        assert_eq!(
            document_start::fix(expected, &cfg),
            None,
            "not idempotent: {expected:?}"
        );
    }
}

#[test]
fn fix_leaves_explicit_or_forbidden_markers_alone() {
    assert_eq!(
        document_start::fix("---\na: 1\n...\n---\nb: 2\n", &Config::new(true)),
        None
    );
    assert_eq!(
        document_start::fix("a: 1\n---\nb: 2\n", &Config::new(false)),
        None
    );
}

#[test]
fn fix_and_format_keep_a_later_document_bom_outside_the_document() {
    let input = "a: 1\n...\n\u{feff}b: 2\n";
    let expected = "---\na: 1\n...\n\u{feff}---\nb: 2\n";
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    std::fs::write(&file, input).unwrap();
    let config = "[lint.rules]\ndocument-start = 'enable'\n";
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _, stderr) = run(Command::new(exe)
        .args(["check", "--fix", "-d", config])
        .arg(&file));
    assert_eq!(code, 0, "fixed output must parse and lint clean: {stderr}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), expected);
    assert_eq!(format_stdin(dir.path(), input), expected);
}

#[test]
fn format_adds_a_marker_to_the_first_of_several_documents() {
    let dir = tempdir().unwrap();
    assert_eq!(
        format_stdin(dir.path(), "a: 1\n---\nb: 2\n...\nc: 3\n"),
        "---\na: 1\n---\nb: 2\n...\n---\nc: 3\n"
    );
}

fn format_stdin(home: &Path, input: &str) -> String {
    let mut child = ryl(home)
        .current_dir(home)
        .args(["format", "-d", "[format]\ndocument-start = 'add'\n"])
        .args(["--stdin-filename", "s.yaml", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
}
