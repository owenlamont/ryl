//! `ryl format` folds over-long plain scalar lines only when `[format] fold-long-lines` is
//! on. `--check` explains each fold it would make and passes lines it cannot fold, which
//! `ryl check` keeps reporting, and `ryl check --fix` never folds.

use std::fs;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

const FOLD: &str = "line-length = 20\n[format]\nfold-long-lines = true\n";
const LONG: &str = "---\nk: aaa bbb ccc ddd eee fff\n";
const FOLDED: &str = "---\nk: aaa bbb ccc ddd\n  eee fff\n";

/// Run `ryl <args> a.yaml` beside a `.ryl.toml` holding `config` (none when `None`),
/// returning the exit code, stdout, stderr and the file afterwards.
fn ryl_on(
    config: Option<&str>,
    input: &str,
    args: &[&str],
) -> (i32, String, String, String) {
    let dir = tempdir().unwrap();
    if let Some(config) = config {
        fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    }
    let file = dir.path().join("a.yaml");
    fs::write(&file, input).unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).args(args).arg(&file));
    (code, stdout, stderr, fs::read_to_string(&file).unwrap())
}

#[test]
fn folding_is_opt_in() {
    for (config, args) in [
        (None, vec!["format"]),
        (Some("line-length = 20\n"), vec!["format"]),
        (None, vec!["format", "-d", "extends: default"]),
    ] {
        let (code, _, stderr, after) = ryl_on(config, LONG, &args);
        assert_eq!(
            (code, after.as_str()),
            (0, LONG),
            "{config:?} {args:?}: {stderr}"
        );
    }
    let (code, _, stderr, after) = ryl_on(Some(FOLD), LONG, &["format"]);
    assert_eq!((code, after.as_str()), (0, FOLDED), "{stderr}");
}

#[test]
fn check_and_diff_explain_folds_and_pass_unfoldable_lines() {
    let url = "url: https://example.com/aaaaaaaaaaaaaaaaaaaa\n";
    let input = format!("{LONG}{url}");
    let (code, _, stderr, after) = ryl_on(Some(FOLD), &input, &["format", "--check"]);
    assert_eq!((code, after.as_str()), (1, input.as_str()), "{stderr}");
    let reported: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("line-length"))
        .collect();
    assert_eq!(reported.len(), 1, "only the folded line: {stderr}");
    assert!(
        reported[0].contains("2:21") && reported[0].contains("(26 > 20"),
        "{stderr}"
    );

    let (code, stdout, stderr, _) = ryl_on(Some(FOLD), &input, &["format", "--diff"]);
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stdout.contains("+k: aaa bbb ccc ddd\n+  eee fff\n"),
        "{stdout}"
    );

    let unfoldable = format!("---\n{url}");
    let (code, stdout, stderr, _) =
        ryl_on(Some(FOLD), &unfoldable, &["format", "--check"]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
}

#[test]
fn a_fold_only_another_pass_makes_necessary_is_still_reported() {
    let config = "line-length = 16\n[format]\nfold-long-lines = true\n";
    let (code, _, stderr, _) = ryl_on(
        Some(config),
        "---\nkey: aaa bbb #c\n",
        &["format", "--check"],
    );
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.lines().any(|line| line.contains("1:1")
            && line.contains("would reformat")
            && line.contains("line-length")),
        "{stderr}"
    );
}

#[test]
fn disabled_lines_stay_whole_and_nothing_is_duplicated() {
    let input = "---\nj: aaa bbb ccc ddd eee fff\n\
                 n: aaa bbb ccc ddd eee fff  # ryl disable-line rule:line-length\n\
                 t: aaa bbb ccc ddd eee fff  # ryl disable-line rule:truthy\n\
                 # ryl disable rule:line-length\nm: aaa bbb ccc ddd eee fff\n";
    let expected = "---\nj: aaa bbb ccc ddd\n  eee fff\n\
                    n: aaa bbb ccc ddd eee fff  # ryl disable-line rule:line-length\n\
                    t: aaa bbb ccc ddd eee fff  # ryl disable-line rule:truthy\n\
                    # ryl disable rule:line-length\nm: aaa bbb ccc ddd eee fff\n";
    let (code, _, stderr, after) = ryl_on(Some(FOLD), input, &["format"]);
    assert_eq!((code, after.as_str()), (0, expected), "{stderr}");
}

#[test]
fn markdown_regions_fold_from_their_own_column() {
    let dir = tempdir().unwrap();
    let config = format!("{FOLD}[files]\nmarkdown = ['*.md']\n");
    fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    let file = dir.path().join("doc.md");
    let fence = "   ```yaml\n   k: aaa bbb ccc ddd eee fff\n   ```\n";
    fs::write(&file, format!("{LONG}---\n\n{fence}")).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).arg("format").arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        format!(
            "{FOLDED}---\n\n   ```yaml\n   k: aaa bbb ccc ddd\n     eee fff\n   ```\n"
        )
    );
}

#[test]
fn check_reports_long_lines_and_check_fix_never_folds() {
    let config = format!("{FOLD}[lint.rules]\nline-length = 'enable'\n");
    let (code, _, stderr, after) = ryl_on(Some(&config), LONG, &["check", "--fix"]);
    assert_eq!((code, after.as_str()), (1, LONG), "{stderr}");
    assert!(
        stderr.contains("2:21") && stderr.contains("line-length"),
        "{stderr}"
    );
}
