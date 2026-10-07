//! `ryl check <paths>` must be equivalent to the bare `ryl <paths>` lint form: same diagnostics,
//! exit codes, `--fix`/`--diff`/`--list-files`/stdin behaviour, and `--format`/`--output-file`
//! handling. The one difference is the bare form's deprecation warning: a single stderr line
//! ahead of everything else, never on stdout, suppressed by `--no-warnings`. Parity tests run
//! the same args both ways and assert exactly that difference.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run, ryl};

/// An inline TOML config (carried via `-d`, so config discovery is bypassed and the tests need
/// no `HOME` isolation) enabling one deterministic error-level rule.
const CFG: &str = "lint.rules.trailing-spaces = \"enable\"";

fn exe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ryl"))
}

const DEPRECATION: &str =
    "warning: bare `ryl <paths>` is deprecated; use `ryl check <paths>` instead\n";

/// Run identical lint args bare and under `check`, asserting `assert_bare_matches_check`.
/// Returns the `check` result for further assertions.
fn assert_parity(home: &Path, args: &[&str]) -> (i32, String, String) {
    let bare = run(ryl(home).args(args));
    let checked = run(ryl(home).arg("check").args(args));
    assert_bare_matches_check(&bare, &checked);
    checked
}

/// Same exit code and stdout; bare stderr is `check`'s with the deprecation line prepended.
fn assert_bare_matches_check(
    bare: &(i32, String, String),
    checked: &(i32, String, String),
) {
    assert_eq!(bare.0, checked.0, "exit codes must match");
    assert_eq!(bare.1, checked.1, "stdout must match: {:?}", bare.1);
    assert_eq!(
        bare.2.strip_prefix(DEPRECATION),
        Some(checked.2.as_str()),
        "bare stderr must be check's plus one leading deprecation line: {:?}",
        bare.2
    );
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
    let stdout = String::from_utf8_lossy_owned(out.stdout);
    let stderr = String::from_utf8_lossy_owned(out.stderr);
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
    assert_bare_matches_check(&bare, &checked);
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

    let bare =
        run(ryl(dir.path()).args(["-d", CFG, "--fix", bare_file.to_str().unwrap()]));
    let checked = run(ryl(dir.path()).args([
        "check",
        "-d",
        CFG,
        "--fix",
        check_file.to_str().unwrap(),
    ]));

    assert_bare_matches_check(&bare, &checked);
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
    assert_bare_matches_check(&bare, &checked);
}

#[test]
fn check_matches_bare_on_stdin_filename() {
    let stdin_args = ["--stdin-filename", "embedded.yaml", "-d", CFG];
    let bare = run_with_stdin(exe().arg("-").args(stdin_args), b"a: 1 \n");
    let checked =
        run_with_stdin(exe().arg("check").arg("-").args(stdin_args), b"a: 1 \n");
    assert_bare_matches_check(&bare, &checked);
    let (_, stdout, stderr) = checked;
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
fn no_warnings_suppresses_the_deprecation_warning() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "a: 1 \n").unwrap();
    let args = ["-d", CFG, "--no-warnings", file.to_str().unwrap()];
    let bare = run(ryl(dir.path()).args(args));
    let checked = run(ryl(dir.path()).arg("check").args(args));
    assert_eq!(
        bare, checked,
        "--no-warnings leaves bare identical to check"
    );
    assert_eq!(bare.0, 1, "the error-level diagnostic still fails the run");
}

#[test]
fn top_level_meta_actions_do_not_warn() {
    let dir = tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    for args in [
        vec!["--print-toml-config-schema"],
        vec!["--print-yaml-config-schema"],
        vec!["--generate-completions", "bash"],
        vec!["--migrate-configs", "--migrate-root", root],
    ] {
        let (code, stdout, stderr) = run(ryl(dir.path()).args(&args));
        assert_eq!(code, 0, "{args:?} should succeed: {stderr}");
        assert!(
            !stdout.contains("is deprecated") && !stderr.contains("is deprecated"),
            "{args:?} must not warn: stdout={stdout} stderr={stderr}"
        );
    }
}
