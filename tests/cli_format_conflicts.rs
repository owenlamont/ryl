//! `ryl format` warns on stderr, once per config file, about each enabled lint rule whose
//! options would reject what the formatter writes; `ryl check` never does.

use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

const LADDER: &str = "[lint.rules]\nbraces = 'enable'\nbrackets = 'enable'\n\
    commas = 'enable'\ncomments-indentation = 'enable'\ndocument-end = 'enable'\n\
    document-start = 'enable'\nempty-lines = 'enable'\nnew-line-at-end-of-file = 'enable'\n\
    new-lines = 'enable'\ntrailing-spaces = 'enable'\n\
    [lint.rules.comments]\nmin-spaces-from-content = 2\nmax-spaces-from-content = 2\n\
    [lint.rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n\
    allow-double-quotes-for-escaping = true\nallow-quoted-quotes = true\n";

const DOUBLE_LADDER: &str = "[format]\nquote-style = 'double'\n\
    [lint.rules.quoted-strings]\nquote-type = 'double'\nrequired = 'only-when-needed'\n\
    allow-quoted-quotes = true\n";

/// The `warning: the <rule> lint rule…` lines `ryl format` prints for `config` (TOML when
/// `toml`, else inline YAML) over two files in each of two directories it covers.
fn conflicts(config: &str, toml: bool, extra: &[&str]) -> Vec<String> {
    let dir = tempdir().unwrap();
    if toml {
        fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    }
    for sub in ["one", "two"] {
        let sub = dir.path().join(sub);
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("a.yaml"), "---\na: 1\n").unwrap();
        fs::write(sub.join("b.yaml"), "---\nb: 1\n").unwrap();
    }
    let mut cmd = ryl(dir.path());
    cmd.arg("format").args(extra);
    if !toml {
        cmd.args(["-d", config]);
    }
    let (code, stdout, stderr) = run(cmd.arg(dir.path()));
    assert_eq!((code, stdout.as_str()), (0, ""), "{config}: {stderr}");
    stderr
        .lines()
        .filter(|line| line.starts_with("warning: the "))
        .map(str::to_string)
        .collect()
}

fn warned_rules(config: &str, toml: bool) -> Vec<String> {
    conflicts(config, toml, &[])
        .iter()
        .map(|line| line.split(' ').nth(2).unwrap().to_string())
        .collect()
}

#[test]
fn agreeing_configs_are_silent() {
    for (config, toml) in [
        ("extends: default", false),
        ("rules: {new-lines: {type: platform}}", false),
        (
            "rules: {document-start: {present: true}, document-end: {present: false}}",
            false,
        ),
        (LADDER, true),
        (DOUBLE_LADDER, true),
        (
            "[format]\nquote-style = 'preserve'\n[lint.rules]\nquoted-strings = 'enable'\n",
            true,
        ),
        (
            "[format]\ndocument-start = 'preserve'\n[lint.rules.document-start]\npresent = true\n",
            true,
        ),
        ("[lint.rules.document-end]\npresent = false\n", true),
        (
            "[format]\nfold-long-lines = true\n[lint.rules.line-length]\nmax = 5\n",
            true,
        ),
    ] {
        assert_eq!(warned_rules(config, toml), Vec::<String>::new(), "{config}");
    }
}

#[test]
fn each_rejecting_rule_warns_once_naming_the_target() {
    let config = "rules: {quoted-strings: enable, new-lines: {type: dos}, \
                  document-start: {present: false}, braces: {min-spaces-inside: 1}, \
                  brackets: {forbid: true}, commas: {min-spaces-after: 2}, \
                  comments: {min-spaces-from-content: 3}, empty-lines: {max: 1}, \
                  colons: {max-spaces-before: 0, max-spaces-after: 0}, \
                  hyphens: {max-spaces-after: 0}}";
    let warnings = conflicts(config, false, &[]);
    let rules: Vec<&str> = warnings
        .iter()
        .map(|line| line.split(' ').nth(2).unwrap())
        .collect();
    assert_eq!(
        rules,
        [
            "new-lines",
            "comments",
            "commas",
            "braces",
            "brackets",
            "colons",
            "hyphens",
            "quoted-strings",
            "document-start",
            "empty-lines"
        ],
        "{warnings:#?}"
    );
    assert!(
        warnings[0].contains("`[format] line-ending = \"lf\"`"),
        "{}",
        warnings[0]
    );
    assert!(
        warnings[1].contains("built-in comments style"),
        "{}",
        warnings[1]
    );
    let toml = "[format]\ndocument-end = 'add'\nquote-style = 'double'\n\
                [lint.rules.document-end]\npresent = false\n\
                [lint.rules.quoted-strings]\nquote-type = 'single'\n";
    let warnings = conflicts(toml, true, &[]);
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    assert!(
        warnings[0].contains("quote-style = \"double\""),
        "{}",
        warnings[0]
    );
    assert!(
        warnings[1].contains("document-end = \"add\""),
        "{}",
        warnings[1]
    );
}

#[test]
fn the_escape_exception_must_be_allowed() {
    let without = LADDER.replace("allow-double-quotes-for-escaping = true\n", "");
    assert_eq!(warned_rules(&without, true), ["quoted-strings"]);
}

#[test]
fn the_quoted_strings_warning_names_the_options_for_the_quote_style() {
    let without = LADDER.replace("allow-quoted-quotes = true\n", "");
    let warnings = conflicts(&without, true, &[]);
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(
        warnings[0].contains(
            "set its options `quote-type = \"single\"`, `required = \"only-when-needed\"`, \
             `allow-double-quotes-for-escaping = true`, `allow-quoted-quotes = true`."
        ),
        "{}",
        warnings[0]
    );
    let mismatched = format!("[format]\nquote-style = 'double'\n{LADDER}");
    let warnings = conflicts(&mismatched, true, &[]);
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(
        warnings[0].contains(
            "set its options `quote-type = \"double\"`, `required = \"only-when-needed\"`, \
             `allow-quoted-quotes = true`."
        ),
        "{}",
        warnings[0]
    );
}

#[test]
fn no_warnings_silences_them_and_check_never_prints_them() {
    let config = "rules: {empty-lines: {max: 1}}";
    assert!(conflicts(config, false, &["--no-warnings"]).is_empty());
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "---\na: 1\n").unwrap();
    let (_, _, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["check", "--fix", "-d", config])
        .arg(&file));
    assert!(!stderr.contains("warning: the"), "{stderr}");
}

#[test]
fn an_unmatched_extra_required_pattern_still_conflicts() {
    let config = format!("{LADDER}extra-required = ['^secret$']\n");
    assert_eq!(warned_rules(&config, true), ["quoted-strings"]);
}

#[test]
fn each_conflicting_config_file_warns_under_its_own_path() {
    let dir = tempdir().unwrap();
    for (sub, gap) in [("one", 3), ("two", 4)] {
        let sub = dir.path().join(sub);
        fs::create_dir(&sub).unwrap();
        let config =
            format!("[lint.rules.comments]\nmin-spaces-from-content = {gap}\n");
        fs::write(sub.join(".ryl.toml"), config).unwrap();
        fs::write(sub.join("a.yaml"), "---\na: 1  # c\n").unwrap();
    }
    let (code, _, stderr) = run(ryl(dir.path()).arg("format").arg(dir.path()));
    assert_eq!(code, 0, "{stderr}");
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("warning: "))
        .collect();
    assert_eq!(warnings.len(), 2, "{stderr}");
    for sub in ["one", "two"] {
        let path = dir.path().join(sub).join(".ryl.toml");
        let prefix = format!("warning: {}: the comments lint rule", path.display());
        assert!(warnings.iter().any(|w| w.starts_with(&prefix)), "{stderr}");
    }
}
