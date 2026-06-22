//! `ryl check <paths>` must be byte-for-byte equivalent to the bare `ryl <paths>` lint form
//! (#369): same diagnostics, exit codes, `--fix`/`--diff`/`--list-files`/stdin behaviour, and
//! `--format`/`--output-file` handling. The bare form keeps working with no deprecation warning
//! this slice. Parity tests run the same args both ways and assert identical output.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run, ryl};

/// A yamllint-compatible YAML config (carried via `-d`, so config discovery is bypassed and the
/// tests need no `HOME` isolation) enabling one deterministic error-level rule.
const CFG: &str = "rules: {trailing-spaces: enable}";

fn exe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ryl"))
}

/// Run identical lint args bare and under `check`, asserting both yield the same
/// `(exit code, stdout, stderr)`. Returns the shared result for further assertions.
fn assert_parity(home: &Path, args: &[&str]) -> (i32, String, String) {
    let bare = run(ryl(home).args(args));
    let mut checked_args = vec!["check"];
    checked_args.extend_from_slice(args);
    let checked = run(ryl(home).args(&checked_args));
    assert_eq!(
        bare, checked,
        "`ryl {args:?}` and `ryl check {args:?}` must be identical"
    );
    bare
}

fn run_with_stdin(cmd: &mut Command, input: &[u8]) -> (i32, String, String) {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ryl");
    if let Err(error) = child.stdin.as_mut().expect("stdin").write_all(input) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let out = child.wait_with_output().expect("wait");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn check_matches_bare_on_clean_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ok.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    let (code, stdout, stderr) =
        assert_parity(dir.path(), &["-d", CFG, file.to_str().unwrap()]);
    assert_eq!(code, 0, "clean file should pass");
    assert!(stdout.is_empty() && stderr.is_empty(), "no diagnostics");
}

#[test]
fn check_matches_bare_on_violations() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "a: 1 \n").unwrap();
    let (code, stdout, stderr) =
        assert_parity(dir.path(), &["-d", CFG, file.to_str().unwrap()]);
    assert_eq!(code, 1, "trailing space is an error");
    let out = command_output(&stdout, &stderr);
    assert!(out.contains("1:5"), "expected line:col 1:5: {out}");
    assert!(out.contains("trailing-spaces"), "expected rule id: {out}");
}

#[test]
fn check_matches_bare_on_list_files() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ok.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    let (code, stdout, _) = assert_parity(
        dir.path(),
        &["-d", CFG, "--list-files", file.to_str().unwrap()],
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("ok.yaml"), "listed file: {stdout}");
}

#[test]
fn check_matches_bare_on_diff_without_mutating() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "a: 1 \n").unwrap();
    let (code, stdout, _) =
        assert_parity(dir.path(), &["-d", CFG, "--diff", file.to_str().unwrap()]);
    assert_eq!(code, 1, "a file would change");
    assert!(
        stdout.contains("-a: 1 ") && stdout.contains("+a: 1"),
        "unified diff body: {stdout}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "a: 1 \n",
        "--diff must not write"
    );
}

/// The load-bearing case: `--format`/`--output-file` order is recovered from clap arg indices,
/// which under `check` live in the subcommand's `ArgMatches`, not the root's. A console + a
/// gitlab report file in one run must come out identical for both invocation forms.
#[test]
fn check_matches_bare_on_multi_format_outputs() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "a: 1 \n").unwrap();
    let bare_report = dir.path().join("bare.json");
    let check_report = dir.path().join("check.json");

    let bare = run(ryl(dir.path()).args([
        "-d",
        CFG,
        "--format",
        "auto",
        "--format",
        "gitlab",
        "-o",
        bare_report.to_str().unwrap(),
        file.to_str().unwrap(),
    ]));
    let checked = run(ryl(dir.path()).args([
        "check",
        "-d",
        CFG,
        "--format",
        "auto",
        "--format",
        "gitlab",
        "-o",
        check_report.to_str().unwrap(),
        file.to_str().unwrap(),
    ]));
    assert_eq!(bare, checked, "console output must match");
    assert_eq!(
        fs::read_to_string(&bare_report).unwrap(),
        fs::read_to_string(&check_report).unwrap(),
        "gitlab report must match"
    );
}

#[test]
fn check_fix_matches_bare_fix() {
    let dir = tempdir().unwrap();
    let bare_file = dir.path().join("bare.yaml");
    let check_file = dir.path().join("check.yaml");
    fs::write(&bare_file, "a: 1 \n").unwrap();
    fs::write(&check_file, "a: 1 \n").unwrap();

    let (bare_code, _bo, bare_err) =
        run(ryl(dir.path()).args(["-d", CFG, "--fix", bare_file.to_str().unwrap()]));
    let (check_code, _co, check_err) = run(ryl(dir.path()).args([
        "check",
        "-d",
        CFG,
        "--fix",
        check_file.to_str().unwrap(),
    ]));

    assert_eq!(bare_code, check_code, "fix exit codes match");
    assert_eq!(bare_err, check_err, "fix summary matches");
    assert_eq!(
        fs::read_to_string(&check_file).unwrap(),
        "a: 1\n",
        "check --fix removed the trailing space"
    );
    assert_eq!(
        fs::read_to_string(&bare_file).unwrap(),
        fs::read_to_string(&check_file).unwrap(),
        "both forms wrote identical content"
    );
}

#[test]
fn check_matches_bare_on_stdin() {
    let bare = run_with_stdin(exe().arg("-").args(["-d", CFG]), b"a: 1 \n");
    let checked =
        run_with_stdin(exe().arg("check").arg("-").args(["-d", CFG]), b"a: 1 \n");
    assert_eq!(bare, checked, "stdin lint parity");
}

#[test]
fn check_matches_bare_on_stdin_filename() {
    let stdin_args = ["--stdin-filename", "embedded.yaml", "-d", CFG];
    let bare = run_with_stdin(exe().arg("-").args(stdin_args), b"a: 1 \n");
    let checked =
        run_with_stdin(exe().arg("check").arg("-").args(stdin_args), b"a: 1 \n");
    assert_eq!(bare, checked, "stdin-filename parity");
    let (_, stdout, stderr) = bare;
    assert!(
        command_output(&stdout, &stderr).contains("embedded.yaml"),
        "label uses the stdin filename"
    );
}

#[test]
fn check_help_lists_every_lint_flag() {
    let (code, stdout, stderr) = run(exe().args(["check", "--help"]));
    assert_eq!(code, 0, "check --help should succeed: {stderr}");
    for flag in [
        "--fix",
        "--diff",
        "--list-files",
        "--markdown",
        "--strict",
        "--no-warnings",
        "--stdin-filename",
        "--config-file",
        "--config-data",
        "--format",
        "--output-file",
    ] {
        assert!(
            stdout.contains(flag),
            "check --help missing {flag}: {stdout}"
        );
    }
}

#[test]
fn completions_include_check_subcommand() {
    let (code, stdout, stderr) = run(exe().args(["--generate-completions", "bash"]));
    assert_eq!(code, 0, "completions should succeed: {stderr}");
    assert!(
        stdout.contains("check"),
        "completion script should mention the check subcommand: {stdout}"
    );
}

#[test]
fn bare_lint_emits_no_deprecation_warning() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ok.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    let (code, _stdout, stderr) =
        run(ryl(dir.path()).args(["-d", CFG, file.to_str().unwrap()]));
    assert_eq!(code, 0);
    assert!(
        stderr.is_empty(),
        "bare ryl must stay silent this slice: {stderr}"
    );
}
