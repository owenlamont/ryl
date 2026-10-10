use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use tempfile::tempdir;

mod common;
use common::cli::ryl;
#[path = "common/encoding.rs"]
mod encoding;
use encoding::encoded;

#[test]
fn byte_preserving_overrides_keep_plain_stdin_lint_diffs() {
    let dir = tempdir().unwrap();
    for label in [
        "utf-8",
        "utf8",
        "UTF_8",
        "unicode-1-1-utf-8",
        "unicode11utf8",
        "unicode20utf8",
        "x-unicode20utf8",
        " UTF8 ",
        "latin-1",
        "windows-1252",
        "shift_jis",
        "iso-2022-jp",
    ] {
        let mut command = ryl(dir.path());
        command.env("YAMLLINT_FILE_ENCODING", label).args([
            "check",
            "-d",
            "{rules: {colons: enable}}",
            "--diff",
            "-",
        ]);
        let output = stdin_output(&mut command, b"a:    1\n");
        assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("+a: 1"),
            "{label}: {output:?}"
        );
    }
}

#[test]
fn stateful_overrides_skip_lint_diffs_that_cannot_apply_to_input_bytes() {
    let dir = tempdir().unwrap();
    for (text, expected) in [("a:    1\n", 1), ("a: 1\n", 0)] {
        let mut command = ryl(dir.path());
        command.env("YAMLLINT_FILE_ENCODING", "iso-2022-jp").args([
            "check",
            "-d",
            "{rules: {colons: enable}}",
            "--diff",
            "-",
        ]);
        let input = [b"\x1b(B".as_slice(), text.as_bytes()].concat();
        let output = stdin_output(&mut command, &input);
        assert_eq!(output.status.code(), Some(expected), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("skipped by --diff"));
    }
}

#[test]
fn unicode_override_aliases_preserve_stdin_encoding_and_actual_bom() {
    let dir = tempdir().unwrap();
    for (width, little, labels) in [
        (
            1,
            false,
            vec![
                "utf-8",
                "utf8",
                "UTF_8",
                "utf-8-sig",
                "utf8-sig",
                "unicode-1-1-utf-8",
                "unicode11utf8",
                "unicode20utf8",
                "x-unicode20utf8",
            ],
        ),
        (
            2,
            true,
            vec![
                "utf-16",
                "UTF_16",
                "utf-16le",
                "utf-16-le",
                "utf16le",
                "unicode",
                "ucs-2",
                "csunicode",
                "iso-10646-ucs-2",
                "unicodefeff",
            ],
        ),
        (
            2,
            false,
            vec!["utf-16", "utf-16be", "utf-16-be", "utf16be", "unicodefffe"],
        ),
        (
            4,
            true,
            vec!["utf-32", "UTF_32", "utf-32le", "utf-32-le", "utf32le"],
        ),
        (4, false, vec!["utf-32", "utf-32be", "utf-32-be", "utf32be"]),
    ] {
        for label in labels {
            for bom in [false, true] {
                for input in ["a:    café😀\n", "a: café😀\n"] {
                    let bytes = encoded(input, width, little, bom);
                    let mut command = ryl(dir.path());
                    command
                        .env("YAMLLINT_FILE_ENCODING", label)
                        .args(["format", "-d", "[format]", "-"]);
                    let output = stdin_output(&mut command, &bytes);
                    assert_eq!(output.status.code(), Some(0), "{label}: {output:?}");
                    assert_eq!(
                        output.stdout,
                        encoded("a: café😀\n", width, little, bom),
                        "{label} bom={bom} input={input:?}"
                    );
                    assert_override_previews(
                        dir.path(),
                        label,
                        &bytes,
                        i32::from(input.starts_with("a:    ")),
                        width != 1 || bom,
                    );
                }
            }
        }
    }
}

fn assert_override_previews(
    home: &std::path::Path,
    label: &str,
    bytes: &[u8],
    expected: i32,
    encoded: bool,
) {
    let path = home.join("override.yaml");
    fs::write(&path, bytes).unwrap();
    for mode in ["--check", "--diff"] {
        let mut command = ryl(home);
        command
            .env("YAMLLINT_FILE_ENCODING", label)
            .args(["format", "-d", "[format]", mode, "-"]);
        let output = stdin_output(&mut command, bytes);
        assert_preview(&output, expected, mode, encoded);
        let output = ryl(home)
            .env("YAMLLINT_FILE_ENCODING", label)
            .args(["format", "-d", "[format]", mode])
            .arg(&path)
            .output()
            .unwrap();
        assert_preview(&output, expected, mode, encoded);
    }
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

const ENCODINGS: [(usize, bool, bool); 10] = [
    (1, false, false),
    (1, false, true),
    (2, false, false),
    (2, false, true),
    (2, true, false),
    (2, true, true),
    (4, false, false),
    (4, false, true),
    (4, true, false),
    (4, true, true),
];

fn assert_preview(output: &Output, expected: i32, mode: &str, encoded: bool) {
    assert_eq!(output.status.code(), Some(expected), "{output:?}");
    if expected == 1 {
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("colons"),
            "{output:?}"
        );
    }
    if mode == "--diff" && encoded {
        assert!(
            output.stdout.is_empty(),
            "encoded input has no applicable patch: {output:?}"
        );
    }
}

fn stdin_output(command: &mut Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn encoded_format_previews_detect_changes_in_files_and_stdin() {
    let dir = tempdir().unwrap();
    for (name, dirty, clean, args) in [
        ("input.yaml", "a:    café\n", "a: café\n", vec![]),
        (
            "input.md",
            "```yaml\na:    café\n```\n",
            "```yaml\na: café\n```\n",
            vec!["--markdown"],
        ),
    ] {
        let path = dir.path().join(name);
        for (width, little, bom) in ENCODINGS {
            for (text, expected) in [
                (dirty, 1),
                (clean, 0),
                ("a: [\n", 0),
                ("# ryl disable-file\na:    café\n", 0),
            ] {
                let bytes = encoded(text, width, little, bom);
                fs::write(&path, &bytes).unwrap();
                for mode in ["--check", "--diff"] {
                    let mut command = ryl(dir.path());
                    command.args(["format", "-d", "[format]", mode]).args(&args);
                    let output = command.arg(&path).output().unwrap();
                    assert_preview(&output, expected, mode, width != 1 || bom);
                    let mut command = ryl(dir.path());
                    command
                        .args(["format", "-d", "[format]", mode])
                        .args(&args)
                        .arg("-");
                    let output = stdin_output(&mut command, &bytes);
                    assert_preview(&output, expected, mode, width != 1 || bom);
                }
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn encoding_overrides_are_preserved_by_stdin_and_checked_by_previews() {
    let dir = tempdir().unwrap();
    for label in ["latin-1", "windows-1252"] {
        for input in [b"a:    caf\xe9\n".as_slice(), b"a: caf\xe9\n"] {
            for mode in [None, Some("--check"), Some("--diff")] {
                let mut command = ryl(dir.path());
                command
                    .env("YAMLLINT_FILE_ENCODING", label)
                    .args(["format", "-d", "[format]"])
                    .args(mode)
                    .arg("-");
                let output = stdin_output(&mut command, input);
                let expected = i32::from(mode.is_some() && input == b"a:    caf\xe9\n");
                assert_eq!(output.status.code(), Some(expected), "{label}: {output:?}");
                if mode.is_none() {
                    assert_eq!(output.stdout, b"a: caf\xe9\n", "{label}");
                }
            }
        }
    }
}

#[test]
fn format_previews_count_changes_without_an_applicable_patch() {
    let dir = tempdir().unwrap();
    for (name, input) in [
        ("bare-cr.yaml", b"a:    1\r".as_slice()),
        ("line\nbreak.yaml", b"a:    1\n"),
    ] {
        for mode in ["--check", "--diff"] {
            let mut command = ryl(dir.path());
            command.args([
                "format",
                "-d",
                "[format]",
                mode,
                "--stdin-filename",
                name,
                "-",
            ]);
            let output = stdin_output(&mut command, input);
            assert_preview(&output, 1, mode, true);
        }
    }
}

#[test]
fn formatted_stdin_preserves_encoding_and_bom() {
    let dir = tempdir().unwrap();
    for (width, little, bom) in ENCODINGS {
        for (input, expected, args) in [
            ("a:    café\n", "a: café\n", vec![]),
            ("a: café\n", "a: café\n", vec![]),
            ("a: [\n", "a: [\n", vec![]),
            (
                "# ryl disable-file\na:    café\n",
                "# ryl disable-file\na:    café\n",
                vec![],
            ),
            (
                "```yaml\na:    café\n```\n",
                "```yaml\na: café\n```\n",
                vec!["--markdown"],
            ),
            (
                "a:    café\n",
                "a:    café\n",
                vec!["--stdin-filename", "ignored.yaml"],
            ),
        ] {
            let mut command = ryl(dir.path());
            command
                .args(["format", "-d", "exclude = ['ignored.yaml']\n[format]"])
                .args(args)
                .arg("-");
            let output =
                stdin_output(&mut command, &encoded(input, width, little, bom));
            assert_eq!(output.status.code(), Some(0), "{output:?}");
            assert_eq!(
                output.stdout,
                encoded(expected, width, little, bom),
                "width={width} little={little} bom={bom} input={input:?}"
            );
        }
    }
}
