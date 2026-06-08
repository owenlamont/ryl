//! `--diff` previews safe fixes as a unified diff without writing.
//!
//! Behaviour mirrors `ruff check --diff`: the diff goes to stdout, the exit code is
//! `1` iff some file would change and `0` otherwise, and remaining *unfixable*
//! findings are neither printed nor counted (a file that only trips an unfixable rule
//! produces no diff and exits `0`, unlike a plain lint run). Files that cannot be
//! parsed — or, on Unix, symlinks — are skipped with a stderr notice and do not
//! affect the exit code.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

mod common;
use common::cli::run;

const TRAILING: &str = "rules: {trailing-spaces: enable}";

fn run_with_stdin(cmd: &mut Command, input: &[u8]) -> (i32, String, String) {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ryl");
    if let Err(error) = child.stdin.as_mut().expect("stdin").write_all(input) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "write stdin: {error}"
        );
    }
    let out = child.wait_with_output().expect("wait");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn diff_prints_unified_diff_and_leaves_file_unchanged() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    let original = "key:   value  \n";
    fs::write(&file, original).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("-d")
        .arg(TRAILING)
        .arg(&file));

    assert_eq!(code, 1, "a pending fix must exit 1: {stderr}");
    assert!(stdout.contains("@@"), "expected a hunk header: {stdout}");
    assert!(
        stdout.contains("-key:   value  ") && stdout.contains("+key:   value"),
        "expected the trailing-space removal in the diff: {stdout}"
    );
    assert!(
        stdout.contains(&format!("--- {}", file.display()))
            && stdout.contains(&format!("+++ {}", file.display())),
        "diff header must carry the file path on both sides: {stdout}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original,
        "--diff must never write the file"
    );
}

#[test]
fn diff_clean_file_exits_zero_with_empty_stdout() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("clean.yaml");
    fs::write(&file, "key: value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("-d")
        .arg(TRAILING)
        .arg(&file));

    assert_eq!(code, 0, "a clean file must exit 0: {stderr}");
    assert!(stdout.is_empty(), "no diff expected: {stdout}");
}

#[test]
fn diff_ignores_remaining_unfixable_findings() {
    // The crux of mirroring `ruff check --diff`: an unfixable-only finding
    // (octal-values has no safe fix) yields no diff, so --diff exits 0 even though a
    // plain lint run would exit 1 on the same file.
    let dir = tempdir().unwrap();
    let file = dir.path().join("octal.yaml");
    fs::write(&file, "port: 010\n").unwrap();
    let cfg = "rules: {octal-values: {forbid-implicit-octal: true}}";

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (diff_code, stdout, _e) = run(Command::new(exe)
        .arg("--diff")
        .arg("-d")
        .arg(cfg)
        .arg(&file));
    assert_eq!(diff_code, 0, "no safe fix means no diff and exit 0");
    assert!(
        stdout.is_empty(),
        "unfixable findings must not diff: {stdout}"
    );

    let (lint_code, ..) = run(Command::new(exe).arg("-d").arg(cfg).arg(&file));
    assert_eq!(
        lint_code, 1,
        "plain lint still reports the unfixable finding"
    );
}

#[test]
fn diff_conflicts_with_fix() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("--fix")
        .arg("-d")
        .arg(TRAILING)
        .arg(&file));

    assert_eq!(code, 2, "--diff and --fix are mutually exclusive: {stderr}");
    assert!(
        stderr.contains("cannot be used with"),
        "expected a clap conflict error: {stderr}"
    );
}

#[test]
fn diff_skips_unparsable_file_with_notice() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("broken.yaml");
    let original = "[1, 2\n[3, 4\n";
    fs::write(&file, original).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("-d")
        .arg(TRAILING)
        .arg(&file));

    assert_eq!(code, 0, "an unparsable file yields no diff: {stderr}");
    assert!(stdout.is_empty(), "no diff for unparsable input: {stdout}");
    assert!(
        stderr.contains("skipped by --diff"),
        "the user must learn why the file was skipped: {stderr}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original,
        "a skipped file is left untouched"
    );
}

#[test]
fn diff_reads_stdin_with_default_label() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("--diff")
            .arg("-d")
            .arg(TRAILING)
            .arg("-"),
        b"key:   value  \n",
    );

    assert_eq!(code, 1, "stdin with a pending fix exits 1: {stderr}");
    assert!(
        stdout.contains("--- <stdin>") && stdout.contains("+++ <stdin>"),
        "stdin diff is labelled <stdin>: {stdout}"
    );
}

#[test]
fn diff_reads_stdin_with_filename_label() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, _stderr) = run_with_stdin(
        Command::new(exe)
            .arg("--diff")
            .arg("--stdin-filename")
            .arg("foo.yaml")
            .arg("-d")
            .arg(TRAILING)
            .arg("-"),
        b"key:   value  \n",
    );

    assert_eq!(code, 1, "stdin with a pending fix exits 1");
    assert!(
        stdout.contains("--- foo.yaml") && stdout.contains("+++ foo.yaml"),
        "--stdin-filename sets the diff label on both header sides: {stdout}"
    );
}

#[test]
fn diff_stdin_unparsable_is_skipped() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("--diff")
            .arg("-d")
            .arg(TRAILING)
            .arg("-"),
        b"[1, 2\n[3, 4\n",
    );

    assert_eq!(code, 0, "unparsable stdin yields no diff: {stderr}");
    assert!(stdout.is_empty(), "no diff for unparsable stdin: {stdout}");
    assert!(
        stderr.contains("skipped by --diff"),
        "skip notice expected for unparsable stdin: {stderr}"
    );
}

#[test]
fn diff_rewrites_yaml_embedded_in_markdown_at_host_level() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "# Title\n\n```yaml\nkey:   value  \n```\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("--markdown")
        .arg("-d")
        .arg(TRAILING)
        .arg(&file));

    assert_eq!(code, 1, "a fixable embedded block exits 1: {stderr}");
    assert!(
        stdout.contains(&format!("--- {}", file.display())),
        "markdown diffs at the host-file level: {stdout}"
    );
    assert!(
        stdout.contains("-key:   value  ") && stdout.contains("+key:   value"),
        "the embedded fix appears in the host diff: {stdout}"
    );
}

#[test]
fn diff_reports_a_file_that_cannot_be_decoded() {
    // A read/decode failure (here invalid UTF-8) must surface as a usage error, not a
    // panic or a silent skip — exercises the error propagation out of the file walk.
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, [0xff, 0xff, 0xfe]).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("-d")
        .arg(TRAILING)
        .arg(&file));

    assert_eq!(code, 2, "an unreadable file is a usage error: {stderr}");
    assert!(
        stdout.is_empty(),
        "no diff for an unreadable file: {stdout}"
    );
    assert!(
        stderr.contains("failed to read"),
        "the read failure must be reported: {stderr}"
    );
}

#[test]
fn diff_reports_stdin_that_cannot_be_decoded() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("--diff")
            .arg("-d")
            .arg(TRAILING)
            .arg("-"),
        &[0xff, 0xff, 0xfe],
    );

    assert_eq!(code, 2, "undecodable stdin is a usage error: {stderr}");
    assert!(
        stderr.contains("failed to read"),
        "the decode failure must be reported: {stderr}"
    );
}

#[test]
fn diff_across_multiple_files_orders_diffs_and_coexists_with_skips() {
    // Pins emit_diff's exit/ordering logic: diffs appear in input order, a clean file
    // emits no block, and an unparsable file is skipped (stderr) without stopping the
    // run or downgrading the exit code from 1.
    let dir = tempdir().unwrap();
    let first = dir.path().join("1-first.yaml");
    let clean = dir.path().join("2-clean.yaml");
    let second = dir.path().join("3-second.yaml");
    let broken = dir.path().join("4-broken.yaml");
    fs::write(&first, "key:   value  \n").unwrap();
    fs::write(&clean, "key: value\n").unwrap();
    fs::write(&second, "name:   other  \n").unwrap();
    fs::write(&broken, "[1, 2\n[3, 4\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("-d")
        .arg(TRAILING)
        .arg(&first)
        .arg(&clean)
        .arg(&second)
        .arg(&broken));

    assert_eq!(code, 1, "a pending fix exits 1 despite the skip: {stderr}");
    let first_at = stdout.find("1-first.yaml").expect("first file diffed");
    let second_at = stdout.find("3-second.yaml").expect("second file diffed");
    assert!(
        first_at < second_at,
        "diffs appear in input order: {stdout}"
    );
    assert!(
        !stdout.contains("2-clean.yaml"),
        "a clean file emits no diff block: {stdout}"
    );
    assert!(
        stderr.contains("4-broken.yaml") && stderr.contains("skipped by --diff"),
        "the unparsable file is skipped on stderr: {stderr}"
    );
}

#[test]
fn diff_markdown_skip_reports_original_line_not_post_fix() {
    // Regression: `--diff` never writes, so a skipped region's notice must use the
    // ORIGINAL line. Block 1's blank lines collapse (empty-lines), shifting block 2 up
    // by two lines in the fixed output; the skip notice for block 2's undefined alias
    // must still point at its original line (10) — as a plain lint does — not the
    // post-fix line (8).
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(
        &file,
        "```yaml\na: 1\n\n\n\nb: 2\n```\n\n```yaml\nc: *missing\n```\n",
    )
    .unwrap();
    let cfg = "rules: {empty-lines: {max: 1}}";

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("--markdown")
        .arg("-d")
        .arg(cfg)
        .arg(&file));

    assert_eq!(code, 1, "block 1 has a fixable diff: {stderr}");
    assert!(stdout.contains("@@"), "host-level diff expected: {stdout}");
    assert!(
        stderr.contains(":10:") && stderr.contains("skipped by --diff"),
        "skip notice must use the original line 10: {stderr}"
    );
    assert!(
        !stderr.contains(":8:"),
        "skip notice must not use the post-fix line 8: {stderr}"
    );
}

#[test]
fn diff_deduplicates_inputs_listed_under_multiple_spellings() {
    // A file reached by several spellings (the directory walk, here twice, plus an
    // explicit arg) must be diffed once — a duplicate patch block would fail to apply
    // on the second copy. Exercises both the candidate-loop and explicit-loop dedup
    // paths and that `./dirty.yaml` (walk) and `dirty.yaml` (explicit) normalize to
    // one identity. Run from the temp dir so the inputs are relative.
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("dirty.yaml"), "key:   value  \n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .current_dir(dir.path())
        .arg("--diff")
        .arg("-d")
        .arg(TRAILING)
        .arg(".")
        .arg(".")
        .arg("dirty.yaml"));

    assert_eq!(code, 1, "a pending fix exits 1: {stderr}");
    assert_eq!(
        stdout.matches("+++ ").count(),
        1,
        "the file must be diffed once despite three spellings: {stdout}"
    );
}

#[cfg(unix)]
#[test]
fn diff_skips_symlink_with_notice() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let target = dir.path().join("real.yaml");
    fs::write(&target, "key:   value  \n").unwrap();
    let link = dir.path().join("link.yaml");
    symlink(&target, &link).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--diff")
        .arg("-d")
        .arg(TRAILING)
        .arg(&link));

    assert_eq!(code, 0, "a skipped symlink produces no diff: {stderr}");
    assert!(stdout.is_empty(), "no diff for a symlink: {stdout}");
    assert!(
        stderr.contains("refusing to follow a symlink for --diff"),
        "expected the symlink skip warning: {stderr}"
    );
}
