use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use ryl::rules::document_start::{self, Config};

use crate::common;
use common::cli::{run, ryl, stdin_output};

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
    let output = stdin_output(
        ryl(home)
            .current_dir(home)
            .args(["format", "-d", "[format]\ndocument-start = 'add'\n"])
            .args(["--stdin-filename", "s.yaml", "-"]),
        input.as_bytes(),
        |result| result.unwrap(),
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn removal_keeps_required_boundaries_and_moves_inline_content() {
    let cases = [
        ("---\na: 1\n", "a: 1\n"),
        ("---\na: 1\n---\nb: 2\n", "a: 1\n---\nb: 2\n"),
        ("# head\n---\na: 1\n", "# head\na: 1\n"),
        ("--- |\n  text\n", "|\n  text\n"),
        ("--- !tag value\n", "!tag value\n"),
        ("--- &a [value, *a]\n", "&a [value, *a]\n"),
        ("--- # comment\na: 1\n", "# comment\na: 1\n"),
        ("--- value\n", "value\n"),
        ("--- ---\n", "--- ---\n"),
        ("--- ...\n", "--- ...\n"),
        ("\u{feff}---\na: 1\n", "\u{feff}a: 1\n"),
        ("---\r\na: 1\r\n", "a: 1\r\n"),
        ("---\ra: 1\r", "a: 1\r"),
        ("---\n...\n", "---\n...\n"),
        ("---\n---\na: 1\n", "---\n---\na: 1\n"),
        ("...\n---\na: 1\n", "...\n---\na: 1\n"),
        ("%YAML 1.2\n---\na: 1\n", "%YAML 1.2\n---\na: 1\n"),
        (
            "%TAG !e! tag:example.com,2026:\n---\n!e!v value\n",
            "%TAG !e! tag:example.com,2026:\n---\n!e!v value\n",
        ),
    ];
    let cfg = ryl::config::YamlLintConfig::from_toml_str(
        "[format]\ndocument-start = 'remove'\nline-ending = 'auto'\n",
    )
    .unwrap();
    for (input, expected) in cases {
        let fixed = document_start::fix(input, &Config::new(false))
            .unwrap_or_else(|| input.into());
        assert_eq!(fixed, expected, "lint removal: {input:?}");
        assert_eq!(document_start::fix(&fixed, &Config::new(false)), None);
        let formatted = ryl::format::format_str(input, &cfg, Path::new("s.yaml"), &[]);
        let expected_format = if expected.contains('\n') {
            expected.to_string()
        } else {
            expected.replace('\r', "\n")
        };
        assert_eq!(formatted, expected_format, "format removal: {input:?}");
        assert_eq!(
            ryl::format::format_str(&formatted, &cfg, Path::new("s.yaml"), &[]),
            formatted
        );
    }
}

#[test]
fn lint_removes_only_the_optional_marker_and_reports_the_separator() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("s.yaml");
    std::fs::write(&file, "--- # comment\na: 1\n---\nb: 2\n").unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path())
        .args([
            "check",
            "--fix",
            "-d",
            "[lint.rules.document-start]\npresent = false\n",
        ])
        .arg(&file));
    assert_eq!(code, 1, "{stdout}\n{stderr}");
    assert_eq!(
        std::fs::read_to_string(file).unwrap(),
        "# comment\na: 1\n---\nb: 2\n"
    );
    assert!(
        format!("{stdout}{stderr}").contains("document-start"),
        "{stdout}{stderr}"
    );
}

#[test]
fn markdown_retains_front_matter_and_fenced_document_markers() {
    let dir = tempdir().unwrap();
    let input = "---\na: 1\n---\n\n```yaml\n---\nb: 2\n```\n";
    for mode in ["format", "check"] {
        let mut command = ryl(dir.path());
        command.args([mode, "--markdown", "--diff", "-d", "[format]\ndocument-start = 'remove'\n[lint.rules.document-start]\npresent = false\n", "-"]);
        let output =
            stdin_output(&mut command, input.as_bytes(), |result| result.unwrap());
        assert!(output.status.success(), "{:?}", output);
        assert!(output.stdout.is_empty(), "{:?}", output);
    }
}

#[test]
fn only_document_start_accepts_the_remove_target() {
    for key in ["document-end", "comment-starting-space"] {
        assert!(
            ryl::config::YamlLintConfig::from_toml_str(&format!(
                "[format]\n{key} = 'remove'\n"
            ))
            .is_err()
        );
    }
}

#[test]
fn removal_normalizes_blank_lines_in_one_format_pass() {
    for newline in ["\n", "\r\n"] {
        for settings in ["", "max-blank-lines = 0\n", "max-blank-lines = 4\n"] {
            let cfg = ryl::config::YamlLintConfig::from_toml_str(&format!(
                "[format]\ndocument-start = 'remove'\nline-ending = 'auto'\n{settings}"
            ))
            .unwrap();
            for (input, expected) in [
                ("---\n\n\na: 1\n", "a: 1\n"),
                (
                    "---\n# c\n\na: 1\n",
                    if settings.contains("= 0") {
                        "# c\na: 1\n"
                    } else {
                        "# c\n\na: 1\n"
                    },
                ),
            ] {
                let input = input.replace('\n', newline);
                let expected = expected.replace('\n', newline);
                let once =
                    ryl::format::format_str(&input, &cfg, Path::new("s.yaml"), &[]);
                assert_eq!(once, expected, "{settings:?} {input:?}");
                assert_eq!(
                    ryl::format::format_str(&once, &cfg, Path::new("s.yaml"), &[]),
                    once
                );
            }
        }
    }
}
