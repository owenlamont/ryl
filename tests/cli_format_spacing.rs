//! `ryl format` respaces `colons` and `hyphens` in place, closing the gap a compact
//! collection's indentation hangs on by re-indenting it, and names each document it cannot
//! re-indent on stderr without failing the run or `--check`; `ryl check --fix` respaces to
//! the lint tolerance.

use std::fs;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

const DIRTY: &str = "---\na :  1\nlist:\n  -   x\n";
const COMPACT: &str = "---\nseq:\n  -   a: 1\n      b: 2\n? k\n:   - x\n    - y\n";
const UNFOLLOWABLE: &str = "k:\n    a: 1\n---\n: v\n";

fn run_on(input: &str, args: &[&str]) -> (i32, String, String) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, input).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).args(args).arg(&file));
    (code, stderr, fs::read_to_string(&file).unwrap())
}

#[test]
fn format_respaces_and_reindents_compact_collections() {
    let (code, stderr, formatted) = run_on(DIRTY, &["format"]);
    assert_eq!(
        (code, formatted.as_str()),
        (0, "---\na: 1\nlist:\n  - x\n"),
        "{stderr}"
    );
    let (code, stderr, formatted) = run_on(COMPACT, &["format"]);
    assert_eq!(
        (code, formatted.as_str(), stderr.as_str()),
        (0, "---\nseq:\n  - a: 1\n    b: 2\n? k\n: - x\n  - y\n", "")
    );
}

#[test]
fn format_check_fails_only_on_what_it_would_change() {
    let (code, stderr, _) = run_on(UNFOLLOWABLE, &["format", "--check"]);
    assert!(
        stderr.contains("cannot re-indent this document"),
        "{stderr}"
    );
    let (code_after, ..) = run_on(DIRTY, &["format", "--check"]);
    assert_eq!((code, code_after), (1, 1), "{stderr}");
    let disabled =
        COMPACT.replace("a: 1", "a: 1  # yamllint disable-line rule:hyphens");
    let (code, stderr, _) = run_on(&disabled, &["format", "--check"]);
    assert_eq!((code, stderr.as_str()), (0, ""));
}

#[test]
fn check_fix_respaces_to_the_tolerance() {
    let config = "rules: {colons: {max-spaces-after: 2}, hyphens: enable}";
    let (code, stderr, fixed) = run_on(
        "a:    1\nb:  2\nlist:\n  -   x\n",
        &["check", "--fix", "-d", config],
    );
    assert_eq!(
        (code, fixed.as_str()),
        (0, "a:  1\nb:  2\nlist:\n  - x\n"),
        "{stderr}"
    );
}

#[test]
fn notices_count_lines_in_the_file_each_mode_leaves() {
    let input = "k: 1\n\n\n\n\n---\n: v\n";
    let (_, stderr, _) = run_on(input, &["format", "--check"]);
    assert!(
        stderr.contains("a.yaml:6:1 indentation not fixed"),
        "{stderr}"
    );
    let (_, stderr, formatted) = run_on(input, &["format"]);
    assert_eq!(formatted, "k: 1\n\n\n---\n: v\n");
    assert!(
        stderr.contains("a.yaml:4:1 indentation not fixed"),
        "{stderr}"
    );
}

#[test]
fn markdown_notices_count_lines_in_the_unchanged_host() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[files]\nmarkdown = ['*.md']\n",
    )
    .unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "```yaml\nk: 1\n\n\n\n---\n: v\n```\n").unwrap();
    let (_, _, stderr) = run(ryl(dir.path()).args(["format", "--check"]).arg(&file));
    assert!(
        stderr.contains("doc.md:6:1 indentation not fixed"),
        "{stderr}"
    );
}

#[test]
fn a_dash_join_never_crosses_an_indentation_disable() {
    let disabled = "-\n  k: v  # ryl disable-line rule:indentation";
    for (input, expected) in [
        (format!("{disabled}\n"), format!("{disabled}\n")),
        (
            format!("{disabled}\n  j: x\n"),
            format!("{disabled}\n  j: x\n"),
        ),
        (disabled.to_string(), format!("{disabled}\n")),
        (
            format!("a: 1\n---\n{disabled}\n"),
            format!("a: 1\n---\n{disabled}\n"),
        ),
    ] {
        let (code, stderr, formatted) = run_on(&input, &["format"]);
        assert_eq!(
            (code, formatted.as_str()),
            (0, expected.as_str()),
            "{stderr}"
        );
    }
}

#[test]
fn each_refused_document_is_named_in_line_order() {
    let (_, stderr, _) = run_on(": a\n---\n: b\n", &["format", "--check"]);
    let lines: Vec<&str> = stderr
        .lines()
        .filter_map(|line| line.split_once("a.yaml:").map(|(_, notice)| notice))
        .filter(|notice| notice.contains("indentation not fixed"))
        .collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    assert!(
        lines[0].starts_with("1:1") && lines[1].starts_with("2:1"),
        "{stderr}"
    );
}

#[test]
fn a_comment_keeping_a_mapping_beside_its_dash_is_named() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[format]\ndash-on-own-line = true\n",
    )
    .unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "---\nseq:\n  - k: v  # note\n    j: x\n").unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).arg("format").arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("a.yaml:3:3 hyphens not fixed: cannot move this mapping below"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\nseq:\n  - k: v  # note\n    j: x\n"
    );
}
