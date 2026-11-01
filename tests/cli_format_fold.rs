//! `ryl format` folds over-long plain scalar lines only when `[format] fold-long-lines` is
//! on. `--check` fails on a fold it would make and passes lines it cannot fold, which
//! `ryl check` keeps reporting, and `ryl check --fix` never folds.

use std::fs;

use tempfile::tempdir;

use crate::common;
use common::cli::{run, ryl, ryl_on};

const FOLD: &str = "line-length = 20\n[format]\nfold-long-lines = true\n";
const LONG: &str = "---\nk: aaa bbb ccc ddd eee fff\n";
const FOLDED: &str = "---\nk: aaa bbb ccc ddd\n  eee fff\n";

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
fn check_and_diff_flag_folds_and_pass_unfoldable_lines() {
    let url = "url: https://example.com/aaaaaaaaaaaaaaaaaaaa\n";
    let input = format!("{LONG}{url}");
    let (code, _, stderr, after) = ryl_on(Some(FOLD), &input, &["format", "--check"]);
    assert_eq!((code, after.as_str()), (1, input.as_str()), "{stderr}");
    assert!(
        stderr.ends_with("1 file would be reformatted\n"),
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
    assert_eq!(
        (code, stdout.as_str(), stderr.as_str()),
        (0, "", "1 file already formatted\n")
    );
}

#[test]
fn a_fold_only_another_pass_makes_necessary_is_made() {
    let config = "line-length = 16\n[format]\nfold-long-lines = true\n";
    let (code, _, stderr, after) =
        ryl_on(Some(config), "---\nkey: aaa bbb #c\n", &["format"]);
    assert_eq!(
        (code, after.as_str()),
        (0, "---\nkey: aaa\n  bbb  # c\n"),
        "the comment's new spacing pushes the line past the limit: {stderr}"
    );
}

#[test]
fn a_fold_leaves_the_comment_after_its_scalar_in_place() {
    let input = "---\n- u: x\n  r: aaa bbb ccc ddd eee # c\n  # d\n";
    let (code, _, stderr, after) = ryl_on(Some(FOLD), input, &["format"]);
    let expected = "---\n- u: x\n  r: aaa bbb ccc ddd\n    eee  # c\n  # d\n";
    assert_eq!(
        (code, after.as_str()),
        (0, expected),
        "a trailing comment does not make the continuation content: {stderr}"
    );
    assert_eq!(ryl_on(Some(FOLD), expected, &["format"]).3, expected);
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
fn a_disable_line_guards_only_the_line_it_precedes_after_block_conversion() {
    let block = format!("{FOLD}sequence-style = 'block'\n");
    let own_line = format!("{block}dash-on-own-line = true\n");
    for (config, input, expected) in [
        (
            &block,
            "---\n# ryl disable-line rule:line-length\nk: [aaa bbb ccc ddd eee fff]\n",
            "---\n# ryl disable-line rule:line-length\nk:\n  - aaa bbb ccc ddd\n    eee fff\n",
        ),
        (
            &own_line,
            "---\nk: [a]\nl:\n  - k: v\n# ryl disable-line rule:line-length\nv: aaa bbb ccc ddd eee fff\n",
            "---\nk:\n  - a\nl:\n  -\n    k: v\n# ryl disable-line rule:line-length\nv: aaa bbb ccc ddd eee fff\n",
        ),
    ] {
        let (code, _, stderr, after) = ryl_on(Some(config), input, &["format"]);
        assert_eq!((code, after.as_str()), (0, expected), "{input:?}: {stderr}");
    }
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
