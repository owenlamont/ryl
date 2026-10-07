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
