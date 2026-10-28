//! `ryl format` runs its own passes over the formatting rules whatever the lint config
//! enables, shaped by the `[format]` table, and `--check`/`--diff` report each file it would
//! change.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::common;
use common::cli::{run, ryl, ryl_on, stdin_output};

/// Dirty for ten of the fourteen formatting rules (not `new-lines`, `comments-indentation`
/// or `document-start`), the tenth being its missing final newline.
const DIRTY: &str =
    "k: 'abc'\nm: {  a: 1 ,b: 2  }\nq: \"a: b\"\n'key': x   \nn: 1 # c\n\n\n\n\nz: [ ]";
const FORMATTED: &str =
    "k: abc\nm: {a: 1, b: 2}\nq: 'a: b'\nkey: x\nn: 1  # c\n\n\nz: []\n";

fn format_file(
    config: Option<&str>,
    input: &str,
    args: &[&str],
) -> (i32, String, String, String) {
    ryl_on(config, input, &[&["format"], args].concat())
}

fn assert_format_batch(config: &str, cases: &[(String, String)], commands: &[&[&str]]) {
    let directory = tempdir().unwrap();
    let configuration = directory.path().join("config.toml");
    fs::write(&configuration, config).unwrap();
    for (index, (input, _)) in cases.iter().enumerate() {
        fs::write(directory.path().join(format!("case-{index}.yaml")), input).unwrap();
    }
    for command in commands {
        let (code, _, stderr) = run(ryl(directory.path())
            .args(*command)
            .args(["--no-cache", "-c"])
            .arg(&configuration)
            .arg(directory.path()));
        assert_eq!(code, 0, "{command:?}: {stderr}");
        for (index, (input, expected)) in cases.iter().enumerate() {
            let formatted =
                fs::read_to_string(directory.path().join(format!("case-{index}.yaml")))
                    .unwrap();
            assert_eq!(&formatted, expected, "{command:?}: {input:?}, {config}");
        }
    }
}

#[test]
fn multiline_double_quotes_take_single_style_in_yaml_and_markdown() {
    for newline in ["\n", "\r\n"] {
        let config = format!(
            "[format]\nquote-style = 'single'\nline-ending = '{}'\n",
            if newline == "\n" { "lf" } else { "cr-lf" }
        );
        for (prefix, suffix, args) in [
            ("", "", &[][..]),
            ("text\n```yaml\n", "```\n", &["--markdown"][..]),
        ] {
            let input = format!("{prefix}a: \"#h0w\n\n    #t5988\"\n{suffix}")
                .replace('\n', newline);
            let expected = format!("{prefix}a: '#h0w\n\n    #t5988'\n{suffix}")
                .replace('\n', newline);
            let (code, _, stderr, formatted) = format_file(Some(&config), &input, args);
            assert_eq!(code, 0, "{stderr}");
            assert_eq!(formatted, expected);
            let (code, _, stderr, again) = format_file(Some(&config), &formatted, args);
            assert_eq!(code, 0, "{stderr}");
            assert_eq!(again, expected);
        }
    }
}

#[test]
fn zero_config_formats_every_rule_and_is_idempotent() {
    let (code, stdout, stderr, formatted) = format_file(None, DIRTY, &[]);
    assert_eq!((code, stdout.as_str()), (0, ""), "{stderr}");
    assert_eq!(formatted, FORMATTED);
    let (code, _, stderr, again) = format_file(None, FORMATTED, &[]);
    assert_eq!((code, again.as_str()), (0, FORMATTED), "{stderr}");
}

#[test]
fn reference_block_scalar_spaces_survive_formatting() {
    for input in [
        "a: |+\n  \nb: 1\n",
        "a: |\n  \nb: 1\n",
        "a: >+\n  \nb: 1\n",
        "- |+\n   \n- x\n",
        "|2\n  x\n  ",
        "|-2\n  x\n  ",
        ">2\n  x\n  ",
        "|2+\n  x\n  ",
        "|+2\n  x\n  ",
        "&a |2\n  x\n  ",
        "!foo |2\n  x\n  ",
        "---\n|2\n  x\n  \n...\n---\nq: v\n",
    ] {
        let (code, _, stderr, formatted) = format_file(None, input, &[]);
        assert_eq!(code, 0, "{input:?}: {stderr}");
        assert!(
            formatted.contains("  \n") || formatted.ends_with("  "),
            "{input:?}: {formatted:?}"
        );
    }
}

#[test]
fn refused_scalar_keeps_the_whole_file() {
    for input in [
        "---\n|+\n  x\n\n  #foo\n...\n---\na:  1\n",
        "---\n|+\n  x\n\n  #foo\n...\n%TAG !e! tag:example.com,2020:\n---\nc:  !e!foo hi  \n",
    ] {
        let (code, _, stderr, formatted) = format_file(Some("[format]"), input, &[]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(formatted, input);
        assert!(
            stderr.contains("cannot format this file safely"),
            "{stderr}"
        );
    }
}

#[test]
fn stream_disable_stays_active_after_refused_scalar() {
    let input = "# ryl disable\n---\n|+\n  x\n\n  #foo\n...\n---\nb:  1\n";
    let (code, _, stderr, formatted) = format_file(Some("[format]"), input, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(formatted, input);
}

#[test]
fn nested_explicit_scalar_disposable_terminal_indent_formats_away() {
    let input = "a:\n  |2\n    x\n  ";
    let (code, _, stderr, formatted) = format_file(Some("[format]"), input, &[]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(formatted, "a:\n  |2\n    x\n");
}

#[test]
fn empty_block_scalar_headers_are_trimmed_and_terminated() {
    for newline in ["\n", "\r\n"] {
        for add_end in [false, true] {
            let config = format!(
                "[format]\nline-ending = '{}'\ndocument-end = '{}'\n",
                if newline == "\n" { "lf" } else { "cr-lf" },
                if add_end { "add" } else { "preserve" },
            );
            let mut cases = Vec::new();
            for header in ["|", ">", "|-", ">-", "|+", ">+", "|2", ">2-", "|+2"] {
                for (prefix, suffix) in [
                    ("a: ", ""),
                    ("- ", " \t"),
                    ("--- ", "  # note \t"),
                    ("a: !!str &s ", "  # note \t"),
                ] {
                    let input = format!("{prefix}{header}{suffix}");
                    let expected = format!(
                        "{prefix}{header}{}{newline}{}",
                        suffix.trim_end_matches([' ', '\t']),
                        if add_end {
                            format!("...{newline}")
                        } else {
                            String::new()
                        },
                    );
                    cases.push((input, expected));
                }
            }
            assert_format_batch(
                &config,
                &cases,
                &[&["format"], &["format", "--check"]],
            );
        }
    }
}

#[test]
fn unterminated_empty_block_scalar_indentation_keeps_its_value() {
    let config =
        "[format]\nline-ending = 'auto'\n[lint.rules]\ntrailing-spaces = 'enable'\n";
    let mut cases = Vec::new();
    for header in ["|", ">", "|-", ">-", "|+", ">+", "|4", ">4-", "|+4"] {
        for newline in ["\n", "\r\n", "\r"] {
            for width in [1, 4] {
                let input = format!("a: {header}{newline}{}", " ".repeat(width));
                let ending = if newline == "\r" { "\n" } else { newline };
                let expected = format!(
                    "a: {header}{ending}{}",
                    if header.contains('+') {
                        ending.to_string()
                    } else {
                        String::new()
                    }
                );
                cases.push((input, expected));
            }
        }
    }
    assert_format_batch(
        config,
        &cases,
        &[&["format"], &["format", "--check"], &["check"]],
    );
}

#[test]
fn keep_scalar_eof_preserves_its_break_with_document_markers() {
    for newline in ["\n", "\r\n"] {
        for input_marker in [false, true] {
            for add_end in [false, true] {
                let config = format!(
                    "[format]\nline-ending = '{}'\ndocument-end = '{}'\n",
                    if newline == "\n" { "lf" } else { "cr-lf" },
                    if add_end { "add" } else { "preserve" },
                );
                let input = format!(
                    "- |+{newline}   {}",
                    if input_marker {
                        format!("{newline}...{newline}")
                    } else {
                        String::new()
                    },
                );
                let expected = if input_marker {
                    format!("- |+{newline}   {newline}...{newline}")
                } else if add_end {
                    format!("- |+{newline}{newline}...{newline}")
                } else {
                    format!("- |+{newline}{newline}")
                };
                let (code, _, stderr, formatted) =
                    format_file(Some(&config), &input, &[]);
                assert_eq!(
                    (code, formatted.as_str()),
                    (0, expected.as_str()),
                    "{stderr}"
                );
            }
        }
    }
}

#[test]
fn unterminated_block_scalar_bodies_gain_a_final_newline_and_marker() {
    for header in ["|", ">", "|+", ">+", "|2", ">+2"] {
        for body in ["  café  ", "   "] {
            if body == "   " && !header.contains('2') {
                continue;
            }
            let input = format!("a: {header} \t\n{body}");
            let expected = format!("a: {header}\n{body}\n...\n");
            let (code, _, stderr, formatted) =
                format_file(Some("[format]\ndocument-end = 'add'\n"), &input, &[]);
            assert_eq!(code, 0, "{stderr}");
            assert_eq!(formatted, expected, "{input:?}");
        }
    }
}

#[test]
fn block_scalar_blank_indentation_is_removed_in_yaml_and_markdown() {
    for header in ["|", ">", "|-", ">-", "|+", ">+", "|3", ">3-", "|-3", ">+3"] {
        for newline in ["\n", "\r\n"] {
            let config = format!(
                "indent-width = 3\n[format]\nline-ending = '{}'\n",
                if newline == "\n" { "lf" } else { "cr-lf" }
            );
            let input = format!("a: {header}\n   \n   x\n  \n    \n   y  \n")
                .replace('\n', newline);
            let expected =
                format!("a: {header}\n\n   x\n\n    \n   y  \n").replace('\n', newline);
            for (prefix, ending, args) in [
                ("", "", &[][..]),
                ("---\n", "---\ntext\n", &["--markdown"][..]),
                ("text\n```yaml\n", "```\ntext\n", &["--markdown"][..]),
            ] {
                let prefix = prefix.replace('\n', newline);
                let ending = ending.replace('\n', newline);
                let host = format!("{prefix}{input}{ending}");
                let wanted = format!("{prefix}{expected}{ending}");
                let (code, _, stderr, formatted) =
                    format_file(Some(&config), &host, args);
                assert_eq!(code, 0, "{stderr}");
                assert_eq!(formatted, wanted, "{host:?}");
                let (code, _, stderr, again) =
                    format_file(Some(&config), &formatted, args);
                assert_eq!(code, 0, "{stderr}");
                assert_eq!(again, formatted);
            }
        }
    }
}

#[test]
fn block_scalar_tails_follow_chomping_in_yaml_and_markdown() {
    for header in ["|", ">", "|-", ">-", "|+", ">+"] {
        for suffix in ["", "b: 1\n"] {
            for newline in ["\n", "\r\n"] {
                let config = format!(
                    "[format]\nmax-blank-lines = 0\nline-ending = '{}'\n",
                    if newline == "\n" { "lf" } else { "cr-lf" }
                );
                let input =
                    format!("a: {header}\n  café\n\n\n{suffix}").replace('\n', newline);
                let tail = if header.contains('+') { "\n\n" } else { "" };
                let expected = format!("a: {header}\n  café\n{tail}{suffix}")
                    .replace('\n', newline);
                for (prefix, ending, args) in [
                    ("", "", &[][..]),
                    ("---\n", "---\ntext\n", &["--markdown"][..]),
                    ("text\n```yaml\n", "```\ntext\n", &["--markdown"][..]),
                ] {
                    let prefix = prefix.replace('\n', newline);
                    let ending = ending.replace('\n', newline);
                    let host = format!("{prefix}{input}{ending}");
                    let wanted = format!("{prefix}{expected}{ending}");
                    let (code, _, stderr, formatted) =
                        format_file(Some(&config), &host, args);
                    assert_eq!(code, 0, "{stderr}");
                    assert_eq!(formatted, wanted, "{host:?}");
                    let (code, _, stderr, again) =
                        format_file(Some(&config), &formatted, args);
                    assert_eq!(code, 0, "{stderr}");
                    assert_eq!(again, formatted);
                }
            }
        }
    }
}

#[test]
fn each_format_key_changes_the_output() {
    let native = if cfg!(windows) { "\r\n" } else { "\n" };
    let cases = [
        (
            "quote-style = 'double'",
            "q: 'a: b'\n",
            "q: \"a: b\"\n".to_string(),
        ),
        (
            "quote-style = 'preserve'",
            "k: 'abc'\n",
            "k: 'abc'\n".to_string(),
        ),
        (
            "line-ending = 'cr-lf'",
            "a: 1\nb: 2",
            "a: 1\r\nb: 2\r\n".to_string(),
        ),
        (
            "line-ending = 'native'",
            "a: 1\r\n",
            format!("a: 1{native}"),
        ),
        (
            "document-start = 'add'",
            "a: 1\n",
            "---\na: 1\n".to_string(),
        ),
        ("document-end = 'add'", "a: 1\n", "a: 1\n...\n".to_string()),
        (
            "brace-spacing = true",
            "a: {b: 1}\ne: {}\nf: { }\nl: [1, 2]\nm: {\n  x: 1\n }\n",
            "a: { b: 1 }\ne: {}\nf: {}\nl: [1, 2]\nm: {\n  x: 1\n }\n".to_string(),
        ),
        (
            "comment-spacing = 1",
            "a: 1   # c\n",
            "a: 1 # c\n".to_string(),
        ),
        (
            "comment-spacing = 3",
            "a: 1 # c\n",
            "a: 1   # c\n".to_string(),
        ),
        (
            "comment-starting-space = 'preserve'",
            "#c\na: 1  #c\n",
            "#c\na: 1  #c\n".to_string(),
        ),
        (
            "max-blank-lines = 0",
            "a: 1\n\nb: 2\n",
            "a: 1\nb: 2\n".to_string(),
        ),
        (
            "max-blank-lines = 1",
            "a: 1\n\n\nb: 2\n",
            "a: 1\n\nb: 2\n".to_string(),
        ),
    ];
    for (key, input, expected) in cases {
        let config = format!("[format]\n{key}\n");
        let (code, _, stderr, formatted) = format_file(Some(&config), input, &[]);
        assert_eq!(
            (code, formatted.as_str()),
            (0, expected.as_str()),
            "{key}: {stderr}"
        );
    }
}

#[test]
fn format_table_rejects_unknown_keys_and_values() {
    for config in [
        "[format]\nindent = 2\n",
        "[format]\nquote-style = 'backtick'\n",
        "[format]\ncomment-spacing = 0\n",
        "[format]\nmax-blank-lines = 256\n",
    ] {
        let (code, _, stderr, formatted) = format_file(Some(config), DIRTY, &[]);
        assert_eq!((code, formatted.as_str()), (2, DIRTY), "{config}: {stderr}");
    }
}

#[test]
fn lint_selection_fix_policy_and_ignores_never_gate_formatting() {
    let config = "[lint]\nfixable = []\nunfixable = ['braces']\n\
                  per-file-ignores = { '*.yaml' = ['ALL'] }\n\
                  [[lint.per-line-ignores]]\nregex = 'm:'\nrules = ['ALL']\n\
                  [lint.rules]\nbraces = 'disable'\nnew-lines = { type = 'dos' }\n";
    let (code, _, stderr, formatted) = format_file(Some(config), DIRTY, &[]);
    assert_eq!((code, formatted.as_str()), (0, FORMATTED), "{stderr}");
}

#[test]
fn a_yaml_config_formats_with_the_default_targets() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, DIRTY).unwrap();
    let (code, _, stderr) = run(ryl(dir.path())
        .args(["format", "-d", "rules: {}"])
        .arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(fs::read_to_string(&file).unwrap(), FORMATTED);
}

#[test]
fn inline_directives_are_honoured() {
    let cases = [
        (
            "# yamllint disable-file\nk: 'abc'\n",
            "# yamllint disable-file\nk: 'abc'\n",
        ),
        (
            "---\n# yamllint disable rule:quoted-strings\nk: 'abc'\na: { b: 1 }\n",
            "---\n# yamllint disable rule:quoted-strings\nk: 'abc'\na: {b: 1}\n",
        ),
    ];
    for (input, expected) in cases {
        let (code, _, stderr, formatted) = format_file(None, input, &[]);
        assert_eq!((code, formatted.as_str()), (0, expected), "{stderr}");
    }
}

#[test]
fn check_shows_the_diff_under_one_finding_and_diff_shows_only_the_diff() {
    for colored in [false, true] {
        let paint = |code: u8, text: &str| {
            if colored {
                format!("  \u{1b}[{code}m{text}\u{1b}[0m\n")
            } else {
                format!("  {text}\n")
            }
        };
        let format = if colored { "colored" } else { "standard" };
        let (code, stdout, stderr, after) =
            format_file(None, DIRTY, &["--check", "--output-format", format]);
        assert_eq!(
            (code, stdout.as_str(), after.as_str()),
            (1, "", DIRTY),
            "{stderr}"
        );
        assert_eq!(
            stderr.matches("unformatted").count(),
            1,
            "one finding per file: {stderr}"
        );
        assert!(
            stderr.contains("1:4")
                && stderr.contains(&paint(36, "@@ -1,10 +1,8 @@"))
                && stderr.contains(&paint(31, "-k: 'abc'"))
                && stderr.contains(&paint(32, "+k: abc"))
                && stderr.ends_with("\n1 file would be reformatted\n"),
            "{format}: {stderr}"
        );
    }
    let (code, stdout, stderr, _) = format_file(None, DIRTY, &["--diff"]);
    assert_eq!(
        (code, stderr.as_str()),
        (1, "1 file would be reformatted\n"),
        "--diff reports no finding"
    );
    assert!(
        stdout.starts_with("--- ") && stdout.contains("\n+k: abc\n"),
        "{stdout}"
    );
    for mode in ["--check", "--diff"] {
        let (code, stdout, stderr, _) = format_file(None, FORMATTED, &[mode]);
        assert_eq!(
            (code, stdout.as_str(), stderr.as_str()),
            (0, "", "1 file already formatted\n"),
            "{mode}"
        );
    }
    let dir = tempdir().unwrap();
    for (name, input) in [("a.yaml", DIRTY), ("b.yaml", DIRTY), ("c.yaml", FORMATTED)] {
        fs::write(dir.path().join(name), input).unwrap();
    }
    for mode in ["--check", "--diff"] {
        let (code, _, stderr) = run(ryl(dir.path())
            .args(["format", "--no-cache", mode])
            .arg(dir.path()));
        assert_eq!(code, 1, "{stderr}");
        assert_eq!(
            stderr.lines().last(),
            Some("2 files would be reformatted, 1 file already formatted"),
            "{mode}: {stderr}"
        );
    }
}

#[test]
fn check_points_at_the_first_change() {
    for (config, input, first_change) in [
        (None, "---\na: 1\nb: 2   \r\n", "3:5"),
        (
            None,
            "---\nk: 'abc'  # yamllint disable-line rule:quoted-strings\nj: 'x'\n",
            "3:4",
        ),
        (None, "a: 1\n\n", "2:1"),
        (None, "a:   1", "1:4"),
        (None, "a: 1\n\n\n\nb: 2\n", "4:1"),
        (
            Some("[format]\ndocument-end = 'add'\n"),
            "---\na: 1\n",
            "3:1",
        ),
    ] {
        let (code, _, stderr, _) = format_file(config, input, &["--check"]);
        assert_eq!(code, 1, "{stderr}");
        assert!(
            stderr
                .lines()
                .any(|line| line.contains(first_change) && line.contains("unformatted")),
            "{input:?} first changes at {first_change}: {stderr}"
        );
    }
}

#[test]
fn check_reports_through_output_format_or_config_output_but_diff_keeps_the_default() {
    let config = "[output]\nparsable = {}\n";
    let (code, _, stderr, _) = format_file(Some(config), DIRTY, &["--check"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("a.yaml:1:4: [error]") && !stderr.contains("@@"),
        "parsable output carries the finding without the diff: {stderr}"
    );
    let gitlab = ["--check", "--output-format", "gitlab"];
    let (code, stdout, stderr, _) = format_file(Some(config), DIRTY, &gitlab);
    assert_eq!(code, 1);
    assert!(
        stdout.starts_with("[{") && !stderr.contains("a.yaml:1:4:"),
        "--output-format overrides [output]: {stdout}{stderr}"
    );
    for args in [&["--diff"][..], &["--diff", "--output-format", "gitlab"]] {
        let (code, stdout, stderr, _) = format_file(Some(config), DIRTY, args);
        assert_eq!(code, 1);
        assert!(
            stdout.starts_with("--- ") && !stderr.contains("a.yaml:1:4:"),
            "{args:?}: {stderr}"
        );
    }
    let config = "[output]\nparsable = { path = 'missing/dir/out.txt' }\n";
    let (code, _, stderr, _) = format_file(Some(config), DIRTY, &["--check"]);
    assert_eq!(code, 2, "an unopenable output target is an error: {stderr}");
}

#[test]
fn check_emits_github_annotations_in_actions() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, DIRTY).unwrap();
    let (code, _, stderr) = run(ryl(dir.path())
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_WORKFLOW", "ci")
        .args(["format", "--check"])
        .arg(&file));
    assert_eq!(code, 1);
    assert!(
        stderr.contains("::error file=")
            && stderr.contains("[unformatted]")
            && !stderr.contains("@@"),
        "{stderr}"
    );
}

#[test]
fn markdown_regions_format_without_file_shape_rules_and_ragged_ones_stay() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[files]\nmarkdown = ['*.md']\n",
    )
    .unwrap();
    let file = dir.path().join("doc.md");
    let ragged = "   ```yaml\n   a: [1,2 ]\n  b: 3\n   ```\n";
    let clean = "```yaml\nc: 1\n```\n";
    let input = format!(
        "---\nk: 'v'\n---\n\n```yaml\na: {{  b: 1 }}\n```\n\n{clean}\n{ragged}"
    );
    fs::write(&file, &input).unwrap();
    let (code, stdout, stderr) =
        run(ryl(dir.path()).args(["format", "--check"]).arg(&file));
    assert_eq!((code, stdout.as_str()), (1, ""), "{stderr}");
    assert!(
        stderr
            .lines()
            .any(|line| line.contains("2:4") && line.contains("unformatted")),
        "the front matter changes first: {stderr}"
    );
    let (code, _, stderr) = run(ryl(dir.path()).arg("format").arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        format!("---\nk: v\n---\n\n```yaml\na: {{b: 1}}\n```\n\n{clean}\n{ragged}")
    );
}

#[test]
fn mixed_newlines_in_markdown_regions_fail_preview_with_a_reason() {
    for input in [
        "---\r\na:   1\nb:   2\r\n---\n",
        "```yaml\na:   1\r\nb:   2\n```\n",
    ] {
        let dir = tempdir().unwrap();
        let file = dir.path().join("doc.md");
        fs::write(&file, input).unwrap();
        for flag in ["--check", "--diff"] {
            let (code, stdout, stderr) = run(ryl(dir.path())
                .args(["format", "--markdown", flag])
                .arg(&file));
            assert_eq!(code, 1, "{flag}: {stderr}");
            assert!(stderr.contains("mixes CRLF and LF"), "{stderr}");
            if flag == "--check" {
                assert!(format!("{stdout}{stderr}").contains("error"));
            }
        }
        let (code, _, stderr) =
            run(ryl(dir.path()).args(["format", "--markdown"]).arg(&file));
        assert_eq!(code, 0, "{stderr}");
        assert!(stderr.contains("mixes CRLF and LF"), "{stderr}");
        assert_eq!(fs::read_to_string(&file).unwrap(), input);
    }
}

#[test]
fn clean_mixed_newline_markdown_region_needs_no_refusal() {
    let input = "```yaml\na: 1\r\nb: 2\n```\n";
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, input).unwrap();
    for args in [
        &["format", "--markdown"][..],
        &["format", "--markdown", "--check"],
        &["format", "--markdown", "--diff"],
    ] {
        let (code, stdout, stderr) = run(ryl(dir.path()).args(args).arg(&file));
        let summary = if args.len() == 3 {
            "1 file already formatted\n"
        } else {
            ""
        };
        assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", summary));
        assert_eq!(fs::read_to_string(&file).unwrap(), input);
    }
}

#[test]
fn markdown_refusals_follow_earlier_in_place_expansion() {
    let front_matter = "---\na: [x, y]\n---\n\n";
    for (fence, reason) in [
        ("```yaml\nb:   1\r\nc:   2\n```\n", "mixes CRLF and LF"),
        (
            "```yaml\nk:\n    v: a\n     ```\n    w:   1\n```\n",
            "would change Markdown fence boundaries",
        ),
    ] {
        let dir = tempdir().unwrap();
        let file = dir.path().join("doc.md");
        fs::write(&file, format!("{front_matter}{fence}")).unwrap();
        let config = "indent-width = 2\n[format]\nsequence-style = 'block'";
        let (code, _, stderr) = run(ryl(dir.path())
            .args(["format", "--markdown", "--check", "-d", config])
            .arg(&file));
        assert_eq!(code, 1, "{stderr}");
        assert!(stderr.contains("doc.md:6:1 skipped by --check"), "{stderr}");
        let (code, _, stderr) = run(ryl(dir.path())
            .args(["format", "--markdown", "-d", config])
            .arg(&file));
        assert_eq!(code, 0, "{stderr}");
        assert!(stderr.contains(reason), "{stderr}");
        assert!(
            stderr.contains("doc.md:8:1 skipped by ryl format"),
            "{stderr}"
        );
        let formatted = fs::read_to_string(&file).unwrap();
        assert_eq!(
            formatted.lines().position(|line| line == "```yaml"),
            Some(6)
        );
        assert!(formatted.ends_with(fence));
    }
}

#[test]
fn ragged_markdown_region_keeps_format_check_red() {
    let input = "- item\n\n  ```yaml\n  a:   1\n b:   2\n  ```\n";
    let (code, _, stderr, formatted) =
        format_file(None, input, &["--markdown", "--check"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr
            .lines()
            .any(|line| line.contains("4:6") && line.contains("unformatted")),
        "the finding sits on the host line of the change: {stderr}"
    );
    assert_eq!(formatted, input);
}

#[test]
fn markdown_fence_boundary_refusal_is_named_without_changing_preview_exit() {
    let input = "```yaml\nk:\n    v: a\n     ```\n    w:   1\n```\n";
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, input).unwrap();
    for args in [
        &["format", "--markdown", "-d", "indent-width = 2"][..],
        &["format", "--markdown", "-d", "indent-width = 2", "--check"],
        &["format", "--markdown", "-d", "indent-width = 2", "--diff"],
    ] {
        let (code, _, stderr) = run(ryl(dir.path()).args(args).arg(&file));
        assert_eq!(code, 0, "{stderr}");
        assert!(
            stderr.contains("fenced YAML block would change Markdown fence boundaries"),
            "{stderr}"
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), input);
    }
}

#[test]
fn ragged_markdown_keep_scalar_region_stays_untouched() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "indent-width = 2\n[format]\n[lint.rules]\ntrailing-spaces = 'enable'\n",
    )
    .unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "```yaml\nq: 1\n```\n\n```yaml\na:\n   b: c\n---\nz: |+\n   ",
    )
    .unwrap();
    let (code, _, stderr) =
        run(ryl(dir.path()).args(["format", "--markdown"]).arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "```yaml\nq: 1\n```\n\n```yaml\na:\n  b: c\n---\nz: |+\n   "
    );
}

#[test]
fn symlinks_and_unparsable_files_are_skipped_and_utf16_keeps_its_encoding() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("target.yaml");
    fs::write(&target, DIRTY).unwrap();
    let bad = dir.path().join("bad.yaml");
    fs::write(&bad, "k: 'a'\nb: [\n").unwrap();
    let wide = dir.path().join("wide.yaml");
    let utf16 = |text: &str| -> Vec<u8> {
        [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect()
    };
    fs::write(&wide, utf16("k: 'abc'\n")).unwrap();
    let mut inputs = vec![bad.clone(), wide.clone()];
    if link(&target, &dir.path().join("link.yaml")) {
        inputs.push(dir.path().join("link.yaml"));
    }
    let (code, _, stderr) = run(ryl(dir.path()).arg("format").args(&inputs));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("bad.yaml:2:4 skipped by ryl format"),
        "{stderr}"
    );
    assert_eq!(fs::read_to_string(&bad).unwrap(), "k: 'a'\nb: [\n");
    assert_eq!(fs::read(&wide).unwrap(), utf16("k: abc\n"));
    assert_eq!(fs::read_to_string(&target).unwrap(), DIRTY);
    let (code, _, stderr) = run(ryl(dir.path()).args(["format", "--check"]).arg(&wide));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("skipped by --check"), "{stderr}");
}

fn link(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    return std::os::windows::fs::symlink_file(target, link).is_ok();
}

#[test]
fn stdin_formats_to_stdout_and_check_reports_it() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("output.toml");
    fs::write(&config, "[output]\nparsable = {}\n").unwrap();
    let config = config.to_str().unwrap();
    let run_stdin = |args: &[&str], input: &[u8]| {
        let out = stdin_output(
            ryl(dir.path())
                .current_dir(dir.path())
                .arg("format")
                .args(args)
                .arg("-"),
            input,
            |_| {},
        );
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    };
    let dirty = DIRTY.as_bytes();
    let (code, stdout, stderr) = run_stdin(&[], dirty);
    assert_eq!((code, stdout.as_str()), (0, FORMATTED), "{stderr}");
    let braces = "rules: {braces: {min-spaces-inside: 1}}";
    let (code, stdout, stderr) = run_stdin(&["--check", "-d", braces], dirty);
    assert_eq!((code, stdout.as_str()), (1, ""), "{stderr}");
    assert!(
        stderr.contains("1:4") && stderr.contains("unformatted"),
        "{stderr}"
    );
    assert!(stderr.contains("warning: the braces lint rule"), "{stderr}");
    let parsable = ["--check", "-c", config, "--stdin-filename", "s.yaml"];
    let (code, _, stderr) = run_stdin(&parsable, dirty);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("s.yaml:1:4: [error]"), "{stderr}");
    let cli = [
        "--check",
        "--output-format",
        "parsable",
        "--stdin-filename",
        "s.yaml",
    ];
    let (code, _, stderr) = run_stdin(&cli, dirty);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("s.yaml:1:4: [error]"), "{stderr}");
    let (code, _, stderr) = run_stdin(&["--check"], &[0xFF, 0xFF, 0xFF]);
    assert_eq!(code, 2, "undecodable stdin is an error: {stderr}");
}

#[test]
fn preview_refuses_an_output_file_that_is_an_input() {
    for input in [DIRTY, FORMATTED] {
        for mode in ["--check", "--diff"] {
            let dir = tempdir().unwrap();
            let file = dir.path().join("a.yaml");
            fs::write(&file, input).unwrap();
            let config = format!(
                "[output]\nparsable = {{ path = '{}' }}\n",
                file.display().to_string().replace('\\', "/")
            );
            let config_file = dir.path().join("cfg.toml");
            fs::write(&config_file, config).unwrap();
            let (code, _, stderr) = run(ryl(dir.path())
                .args(["format", mode, "-c"])
                .arg(&config_file)
                .arg(&file));
            let expected = if mode == "--check" {
                2
            } else {
                u8::from(input == DIRTY).into()
            };
            assert_eq!(code, expected, "{mode}: {stderr}");
            assert_eq!(fs::read_to_string(&file).unwrap(), input, "{mode}");
        }
    }
    let dir = tempdir().unwrap();
    let named = dir.path().join("s.yaml");
    let named = named.to_str().unwrap();
    let config_file = dir.path().join("cfg.toml");
    let config = format!(
        "[output]\nparsable = {{ path = '{}' }}\n",
        named.replace('\\', "/")
    );
    fs::write(&config_file, config).unwrap();
    let out = ryl(dir.path())
        .args(["format", "--check", "--stdin-filename", named, "-c"])
        .arg(&config_file)
        .arg("-")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(!dir.path().join("s.yaml").exists());
}

#[test]
fn keeps_hash_led_lines_of_a_block_scalar_in_place() {
    for input in [
        "k: |\n  |--- |--- |\n  |PUT|x|\n    ### S\n",
        "|\n  literal\n # c\n",
    ] {
        let (code, _, stderr, formatted) = format_file(None, input, &[]);
        assert_eq!((code, formatted.as_str()), (0, input), "{stderr}");
    }
}

#[test]
fn keeps_quotes_on_y_and_n_which_go_yaml_v2_loads_as_booleans() {
    let input = "a: 'y'\nb: \"N\"\n'n': 1\n";
    for (style, expected) in [
        ("single", "a: 'y'\nb: 'N'\n'n': 1\n"),
        ("double", "a: \"y\"\nb: \"N\"\n\"n\": 1\n"),
        ("preserve", input),
    ] {
        let config = format!("[format]\nquote-style = '{style}'\n");
        let (code, _, stderr, formatted) = format_file(Some(&config), input, &[]);
        assert_eq!(
            (code, formatted.as_str()),
            (0, expected),
            "{style}: {stderr}"
        );
    }
}
