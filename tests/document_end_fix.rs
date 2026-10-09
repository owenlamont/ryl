use std::io::Write;
use std::process::Stdio;

use tempfile::tempdir;

use ryl::rules::document_end::{self, Config};

mod common;
use common::cli::ryl;

#[test]
fn fix_ends_every_implicitly_ended_document() {
    let cases = [
        ("a: 1\n---\nb: 2\n", "a: 1\n...\n---\nb: 2\n...\n"),
        ("a: 1\n...\nb: 2\n", "a: 1\n...\nb: 2\n...\n"),
        ("...\na: 1\n", "...\na: 1\n...\n"),
        ("--- a\n--- b\n", "--- a\n...\n--- b\n...\n"),
        ("---\n---\n", "---\n...\n---\n...\n"),
        ("a: 1\n--- # c\nb\n", "a: 1\n...\n--- # c\nb\n...\n"),
        ("a: 1\n# c\n\n---\nb\n", "a: 1\n# c\n\n...\n---\nb\n...\n"),
        ("a: |+\n  x\n\n---\nb\n", "a: |+\n  x\n\n...\n---\nb\n...\n"),
        ("--- |\n  x\n--- b", "--- |\n  x\n...\n--- b\n...\n"),
        ("x: |\n  ---\n", "x: |\n  ---\n...\n"),
        (
            "%YAML 1.2\n---\na: 1\n---\nb\n",
            "%YAML 1.2\n---\na: 1\n...\n---\nb\n...\n",
        ),
        (
            "\u{feff}a: 1\n---\nb: 2\n",
            "\u{feff}a: 1\n...\n---\nb: 2\n...\n",
        ),
        (
            "a: 1\n...\n\u{feff}---\nb: 2\n",
            "a: 1\n...\n\u{feff}---\nb: 2\n...\n",
        ),
        (
            "a: 1\r\n# c\r\n---\r\nb\r\n",
            "a: 1\r\n# c\r\n...\r\n---\r\nb\r\n...\r\n",
        ),
        ("a: 1\r# c\r\r---\rb\r", "a: 1\r# c\r\r...\r---\rb\r...\r"),
    ];
    let cfg = Config::new(true);
    for (input, expected) in cases {
        assert_eq!(
            document_end::fix(input, &cfg).as_deref(),
            Some(expected),
            "input: {input:?}"
        );
        assert!(
            document_end::check(expected, &cfg).is_empty(),
            "not clean: {expected:?}"
        );
    }
}

#[test]
fn fix_inserts_ends_but_skips_the_append_after_an_unterminated_block_scalar() {
    assert_eq!(
        document_end::fix("a: 1\n---\nb: |\n  x", &Config::new(true)).as_deref(),
        Some("a: 1\n...\n---\nb: |\n  x")
    );
}

#[test]
fn fix_leaves_ended_or_forbidden_markers_alone() {
    assert_eq!(
        document_end::fix("a: 1\n...\n---\nb: 2\n...\n", &Config::new(true)),
        None
    );
    assert_eq!(
        document_end::fix("a: 1\n---\nb: 2\n", &Config::new(false)),
        None
    );
}

#[test]
fn format_adds_an_end_to_every_document_of_a_stream() {
    let dir = tempdir().unwrap();
    let mut child = ryl(dir.path())
        .current_dir(dir.path())
        .args(["format", "-d", "[format]\ndocument-end = 'add'\n"])
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
        .write_all(b"a: 1\n---\nb: 2\n...\nc: 3\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "a: 1\n...\n---\nb: 2\n...\nc: 3\n...\n"
    );
}
