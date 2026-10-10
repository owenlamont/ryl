//! `ryl format` runs its own passes over the formatting rules whatever the lint config
//! enables, shaped by the `[format]` table, and `--check`/`--diff` explain each change with
//! the rule that makes it.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

mod common;
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
            "a: {b: 1}\ne: {}\nf: { }\nl: [1, 2]\nm: {\n  x: 1\n}\n",
            "a: { b: 1 }\ne: {}\nf: {}\nl: [1, 2]\nm: {\n  x: 1\n}\n".to_string(),
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
fn check_and_diff_name_the_rule_behind_each_change() {
    let expected = [
        ("1:4", "quoted-strings"),
        ("2:6", "braces"),
        ("2:11", "commas"),
        ("4:1", "quoted-strings"),
        ("4:9", "trailing-spaces"),
        ("5:6", "comments"),
        ("9:1", "empty-lines"),
        ("10:5", "brackets"),
        ("10:7", "new-line-at-end-of-file"),
    ];
    for mode in ["--check", "--diff"] {
        let (code, stdout, stderr, after) = format_file(None, DIRTY, &[mode]);
        assert_eq!((code, after.as_str()), (1, DIRTY), "{mode}: {stderr}");
        for (position, rule) in expected {
            let line = stderr.lines().find(|line| line.contains(position));
            assert!(
                line.is_some_and(|line| line.contains(rule)),
                "{mode}: {rule} at {position}: {stderr}"
            );
        }
        assert_eq!(
            stdout.starts_with("--- "),
            mode == "--diff",
            "{mode}: {stdout}"
        );
    }
    for mode in ["--check", "--diff"] {
        let (code, stdout, stderr, _) = format_file(None, FORMATTED, &[mode]);
        assert_eq!(
            (code, stdout.as_str(), stderr.as_str()),
            (0, "", ""),
            "{mode}"
        );
    }
}

#[test]
fn check_falls_back_per_rule_and_skips_directive_disabled_lines() {
    let (code, _, stderr, _) =
        format_file(None, "---\na: 1\nb: 2   \r\n", &["--check"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.lines().any(|l| l.contains("1:1")
            && l.contains("would reformat")
            && l.contains("new-lines"))
            && stderr
                .lines()
                .any(|l| l.contains("3:5") && l.contains("trailing-spaces")),
        "the mixed ending has no checker diagnostic, so it falls back: {stderr}"
    );
    let config = "[format]\ndocument-end = 'add'\n";
    let (code, _, stderr, _) =
        format_file(Some(config), "---\na: 1\n  # c\nb: 2\n", &["--check"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr
            .lines()
            .any(|l| l.contains("3:3") && l.contains("comments-indentation"))
            && stderr
                .lines()
                .any(|l| l.contains("4:1") && l.contains("document-end")),
        "{stderr}"
    );
    let input = "---\nk: 'abc'  # yamllint disable-line rule:quoted-strings\nj: 'x'\n";
    let (code, _, stderr, _) = format_file(None, input, &["--check"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("3:4") && !stderr.contains("2:4"),
        "{stderr}"
    );
    let unedited = "---\na: {'k':v}\nb: { c: 1 }\n";
    let (code, _, stderr, _) = format_file(None, unedited, &["--check"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("braces"), "{stderr}");
    assert!(!stderr.contains("2:5"), "{stderr}");
    let kept_beside_fixed = "---\na: {'k':v}\nb: 'abc'\n";
    let (code, _, stderr, _) = format_file(None, kept_beside_fixed, &["--check"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("3:4") && !stderr.contains("2:5"),
        "only the quotes format removes are reported: {stderr}"
    );
}

#[test]
fn check_reports_through_output_format_or_config_output_but_diff_keeps_the_default() {
    let config = "[output]\nparsable = {}\n";
    let (code, _, stderr, _) = format_file(Some(config), DIRTY, &["--check"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("a.yaml:2:6: [error]"), "{stderr}");
    let gitlab = ["--check", "--output-format", "gitlab"];
    let (code, stdout, stderr, _) = format_file(Some(config), DIRTY, &gitlab);
    assert_eq!(code, 1);
    assert!(
        stdout.starts_with("[{") && !stderr.contains("a.yaml:2:6:"),
        "--output-format overrides [output]: {stdout}{stderr}"
    );
    for args in [&["--diff"][..], &["--diff", "--output-format", "gitlab"]] {
        let (code, stdout, stderr, _) = format_file(Some(config), DIRTY, args);
        assert_eq!(code, 1);
        assert!(
            stdout.starts_with("--- ") && !stderr.contains("a.yaml:2:6:"),
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
        stderr.contains("::error file=") && stderr.contains("[braces]"),
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
    let reported: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("error"))
        .collect();
    assert_eq!(
        reported.len(),
        3,
        "front matter, fenced block, not ragged: {stderr}"
    );
    assert!(
        reported.iter().all(|line| !line.contains("document-")),
        "{stderr}"
    );
    let (code, _, stderr) = run(ryl(dir.path()).arg("format").arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        format!("---\nk: v\n---\n\n```yaml\na: {{b: 1}}\n```\n\n{clean}\n{ragged}")
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
fn stdin_formats_to_stdout_and_check_explains() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("output.toml");
    fs::write(&config, "[output]\nparsable = {}\n").unwrap();
    let config = config.to_str().unwrap();
    let run_stdin = |args: &[&str], input: &[u8]| {
        let out = stdin_output(
            ryl(dir.path()).arg("format").args(args).arg("-"),
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
        stderr.contains("2:6") && stderr.contains("braces"),
        "{stderr}"
    );
    assert!(stderr.contains("warning: the braces lint rule"), "{stderr}");
    let parsable = ["--check", "-c", config, "--stdin-filename", "s.yaml"];
    let (code, _, stderr) = run_stdin(&parsable, dirty);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("s.yaml:2:6: [error]"), "{stderr}");
    let cli = [
        "--check",
        "--output-format",
        "parsable",
        "--stdin-filename",
        "s.yaml",
    ];
    let (code, _, stderr) = run_stdin(&cli, dirty);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("s.yaml:2:6: [error]"), "{stderr}");
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
fn check_says_when_the_quotes_change_to_avoid_an_escape() {
    let input = "---\na: 'it''s: x'\nb: \"say \\\"hi\\\": x\"\nc: \"it's: x\"\n";
    let (code, _, stderr, _) = format_file(None, input, &["--check"]);
    assert_eq!(code, 1, "{stderr}");
    for (position, message) in [
        ("2:4", "change outer quotes to avoid escaping inner quotes"),
        ("3:4", "string value is not quoted with single quotes"),
    ] {
        let line = stderr.lines().find(|line| line.contains(position));
        assert!(
            line.is_some_and(|line| line.contains(message)),
            "{message} at {position}: {stderr}"
        );
    }
    assert!(!stderr.contains("4:4"), "{stderr}");
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
