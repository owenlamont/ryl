use std::io::Write;
use std::process::Stdio;

use tempfile::tempdir;

use ryl::rules::document_start::{self, Config};

mod common;
use common::cli::ryl;

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
fn format_adds_a_marker_to_the_first_of_several_documents() {
    let dir = tempdir().unwrap();
    let mut child = ryl(dir.path())
        .current_dir(dir.path())
        .args(["format", "--stdin-filename", "s.yaml", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"a: 1\n---\nb: 2\n...\nc: 3\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "---\na: 1\n---\nb: 2\n...\n---\nc: 3\n"
    );
}
