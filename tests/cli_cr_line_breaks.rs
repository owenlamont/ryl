//! End-to-end guards for the unified bare-`\r` (YAML 1.2 line break) handling of
//! issue #284: every rule treats a lone `\r` as a line break, so its line/column
//! counting and `--fix`/`--diff` behaviour match `\r\n`/`\n`. These pin the
//! deliberate divergence from yamllint (whose line layer is `\n`-only) documented
//! in `docs/getting-started/migrating-from-yamllint.md`. Assertions are
//! format-agnostic (bare `line:col` + bare rule id), per the repo testing notes.

use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

fn lint(config: &str, bytes: &[u8]) -> (i32, String) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, bytes).unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-d").arg(config).arg(&file));
    let output = if stderr.is_empty() { stdout } else { stderr };
    (code, output)
}

#[test]
fn trailing_spaces_flags_run_before_a_bare_cr() {
    // `a: 1  \rb: 2\n`: the spaces sit at the end of line 1 (terminated by the bare
    // `\r`); a `\n`-only scanner would treat them as mid-line content and miss them.
    let (code, output) = lint("rules:\n  trailing-spaces: enable\n", b"a: 1  \rb: 2\n");
    assert_eq!(code, 1, "expected a trailing-spaces violation: {output}");
    assert!(output.contains("1:5"), "want line 1 col 5: {output}");
    assert!(
        output.contains("trailing-spaces"),
        "rule id missing: {output}"
    );
}

#[test]
fn new_line_at_end_of_file_accepts_a_trailing_bare_cr() {
    // A bare `\r` is a YAML 1.2 line break, so a file ending in one already ends in a
    // newline (yamllint, `\n`-only, would wrongly report "no new line").
    let (code, output) = lint("rules:\n  new-line-at-end-of-file: enable\n", b"a: 1\r");
    assert_eq!(
        code, 0,
        "a trailing bare CR is a valid EOF newline: {output}"
    );
}

#[test]
fn line_length_measures_to_the_bare_cr_not_across_it() {
    // `aa\raa bb\n` is two short lines (`aa`, `aa bb`); a `\n`-only scanner would see
    // one 8-char line and flag it under max 6.
    let (code, output) = lint(
        "rules:\n  line-length:\n    max: 6\n    allow-non-breakable-words: false\n",
        b"aa\raa bb\n",
    );
    assert_eq!(
        code, 0,
        "bare CR splits the line, so neither exceeds 6: {output}"
    );
}

#[test]
fn empty_lines_counts_blank_runs_made_of_bare_cr() {
    // `a: 1\r\r\r\rb: 2\r`: three blank lines (CR-delimited) between the entries.
    let (code, output) = lint(
        "rules:\n  empty-lines:\n    max: 2\n    max-start: 0\n    max-end: 0\n",
        b"a: 1\r\r\r\rb: 2\r",
    );
    assert_eq!(code, 1, "expected too-many-blank-lines: {output}");
    assert!(
        output.contains("4:1"),
        "want the run's last line (4): {output}"
    );
    assert!(output.contains("empty-lines"), "rule id missing: {output}");
}

#[test]
fn new_lines_flags_a_bare_cr_first_ending() {
    // First line break is a bare `\r`, which is neither `unix` nor `dos`.
    let (code, output) =
        lint("rules:\n  new-lines:\n    type: unix\n", b"a: 1\rb: 2\n");
    assert_eq!(code, 1, "expected wrong-newline violation: {output}");
    assert!(output.contains("1:5"), "want line 1 col 5: {output}");
    assert!(output.contains("new-lines"), "rule id missing: {output}");
}

#[test]
fn fix_rewrites_a_bare_cr_file_in_place() {
    // The user-facing contract: `--fix` handles `\r`-line-ending files. With
    // new-lines (unix) it normalises the bare CRs and strips the trailing space.
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, b"a: 1 \rb: 2\r").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  new-lines:\n    type: unix\n  trailing-spaces: enable\n  new-line-at-end-of-file: enable\n")
        .arg("--fix")
        .arg(&file));
    assert_eq!(code, 0, "fix should succeed: {err}");
    assert_eq!(
        fs::read(&file).unwrap(),
        b"a: 1\nb: 2\n",
        "bare CRs normalised to LF and trailing space stripped"
    );
}

#[test]
fn diff_skips_a_file_ending_in_a_bare_cr() {
    // A unified diff is `\n`-terminated by format, and `similar` counts a trailing
    // `\r` as a terminator, so content ending in a bare `\r` has no applicable patch:
    // `--diff` skips it (→ `--fix`) rather than emit a corrupt diff.
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, b"a: 1 \rb: 2\r").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  trailing-spaces: enable\n")
        .arg("--diff")
        .arg(&file));
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert_eq!(code, 0, "a skip has no exit effect: {output}");
    assert!(
        output.contains("skipped by --diff") && output.contains("carriage return"),
        "expected a bare-CR --diff skip notice: {output}"
    );
}

/// Helper: run `--fix` with an inline config over `input` and return the rewritten
/// bytes. The fix-output line-ending helpers must reuse the file's bare `\r`, not
/// fall back to LF (issue #284) — regressions Codex flagged in the `--fix` paths.
fn fixed_bytes(config: &str, input: &[u8]) -> Vec<u8> {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, input).unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _out, _err) = run(Command::new(exe)
        .arg("-d")
        .arg(config)
        .arg("--fix")
        .arg(&file));
    fs::read(&file).unwrap()
}

#[test]
fn fix_document_start_marker_reuses_a_bare_cr() {
    // Inserting `---` into a `\r`-delimited file must use `\r`, not LF (buffer_newline).
    let out = fixed_bytes(
        "rules:\n  document-start:\n    present: true\n",
        b"a: 1\rb: 2",
    );
    assert_eq!(
        out, b"---\ra: 1\rb: 2",
        "marker line must reuse the bare CR"
    );
}

#[test]
fn fix_appends_a_bare_cr_eof_newline_when_new_lines_is_off() {
    // With no `new-lines` rule, the appended final newline reuses the file's bare `\r`
    // (first_newline) rather than falling back to LF.
    let out = fixed_bytes("rules:\n  new-line-at-end-of-file: enable\n", b"a: 1\rb: 2");
    assert_eq!(out, b"a: 1\rb: 2\r", "EOF newline must reuse the bare CR");
}

#[test]
fn fix_document_end_no_spurious_break_before_marker_on_trailing_cr() {
    // A file already ending in a bare `\r` needs no separator before `...`; a `\n`-only
    // end check would insert a spurious blank line.
    let out = fixed_bytes(
        "rules:\n  document-end:\n    present: true\n",
        b"a: 1\r\nb: 2\r",
    );
    assert_eq!(
        out, b"a: 1\r\nb: 2\r...\r\n",
        "trailing bare CR already terminates the line, so no extra break before ..."
    );
}
