//! `ryl format` respaces `colons` and `hyphens` in place, and names each compact
//! collection it leaves alone on stderr without failing the run or `--check`; `ryl check
//! --fix` respaces to the lint tolerance.

use std::fs;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

const DIRTY: &str = "---\na :  1\nlist:\n  -   x\n";
const COMPACT: &str = "---\nseq:\n-   a: 1\n    b: 2\n? k\n:   - x\n    - y\n";

fn run_on(input: &str, args: &[&str]) -> (i32, String, String) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, input).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).args(args).arg(&file));
    (code, stderr, fs::read_to_string(&file).unwrap())
}

#[test]
fn format_respaces_and_reports_what_it_leaves() {
    let (code, stderr, formatted) = run_on(DIRTY, &["format"]);
    assert_eq!(
        (code, formatted.as_str()),
        (0, "---\na: 1\nlist:\n  - x\n"),
        "{stderr}"
    );
    let (code, stderr, formatted) = run_on(COMPACT, &["format"]);
    assert_eq!((code, formatted.as_str()), (0, COMPACT), "{stderr}");
    let notices: Vec<&str> = stderr
        .lines()
        .filter_map(|line| line.split_once("a.yaml:").map(|(_, notice)| notice))
        .collect();
    assert_eq!(
        notices,
        [
            "3:4 hyphens not fixed: too many spaces after hyphen; respacing would \
             re-indent the block collection after it",
            "6:4 colons not fixed: too many spaces after colon; respacing would \
             re-indent the block collection after it",
        ],
        "{stderr}"
    );
}

#[test]
fn format_check_fails_only_on_what_it_would_change() {
    let (code, stderr, _) = run_on(COMPACT, &["format", "--check"]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("hyphens not fixed"), "{stderr}");
    let (code, stderr, _) = run_on(DIRTY, &["format", "--check"]);
    assert_eq!(code, 1, "{stderr}");
    let disabled =
        COMPACT.replace("a: 1", "a: 1  # yamllint disable-line rule:hyphens");
    let (_, stderr, _) = run_on(&disabled, &["format", "--check"]);
    assert!(!stderr.contains("hyphens not fixed"), "{stderr}");
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
    let input = "-   a: 1\n    b: 2\n";
    let (_, stderr, _) = run_on(input, &["format", "--check"]);
    assert!(stderr.contains("a.yaml:1:4 hyphens not fixed"), "{stderr}");
    let (_, stderr, formatted) = run_on(input, &["format"]);
    assert_eq!(formatted, format!("---\n{input}"));
    assert!(stderr.contains("a.yaml:2:4 hyphens not fixed"), "{stderr}");
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
    fs::write(&file, "```yaml\nk:\n\n\n\n-   x: 1\n    y: 2\n```\n").unwrap();
    let (_, _, stderr) = run(ryl(dir.path()).args(["format", "--check"]).arg(&file));
    assert!(stderr.contains("doc.md:6:4 hyphens not fixed"), "{stderr}");
}
