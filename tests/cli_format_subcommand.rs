//! `ryl format`'s input, stdin, parse-skip and config plumbing, shared with `ryl check
//! --fix`/`--diff`: every mode leaves already-formatted and unparsable inputs byte-for-byte
//! unchanged and exits 0.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

fn exe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ryl"))
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
fn format_help_lists_its_flags_and_completions_include_it() {
    let (code, stdout, stderr) = run(exe().args(["format", "--help"]));
    assert_eq!(code, 0, "format --help should succeed: {stderr}");
    for flag in ["--check", "--diff", "--stdin-filename", "--config-file"] {
        assert!(
            stdout.contains(flag),
            "format --help missing {flag}: {stdout}"
        );
    }
    let (_, completions, _) = run(exe().args(["--generate-completions", "bash"]));
    assert!(
        completions.contains("ryl__subcmd__format"),
        "bash completions should include the format subcommand"
    );
}

#[test]
fn check_and_diff_are_mutually_exclusive() {
    let (code, _, stderr) = run(exe().args(["format", "--check", "--diff", "x.yaml"]));
    assert_eq!(code, 2, "--check with --diff is a usage error: {stderr}");
}

/// A `[format]`-only project config enables no lint rules, which `check` rejects; `format`
/// must run regardless. The Markdown file routes through the embedded-region path.
#[test]
fn every_mode_leaves_formatted_files_unchanged_and_reports_parse_skips() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[files]\nmarkdown = [\"*.md\"]\n\n[format]\n",
    )
    .unwrap();
    let inputs = [
        ("a.yaml", "---\nb: 1\na: [1, 2]\n"),
        ("doc.md", "# Doc\n\n```yaml\nb: 1\n```\n"),
        ("bad.yaml", "a: [\n"),
    ];
    for (name, content) in inputs {
        fs::write(dir.path().join(name), content).unwrap();
    }
    for (mode, label) in [
        (None, "ryl format"),
        (Some("--check"), "--check"),
        (Some("--diff"), "--diff"),
    ] {
        let (code, stdout, stderr) =
            run(ryl(dir.path()).arg("format").args(mode).arg(dir.path()));
        assert_eq!(code, 0, "{label}: nothing to reformat: {stderr}");
        assert!(stdout.is_empty(), "{label}: nothing to print: {stdout}");
        assert!(
            stderr.contains("bad.yaml:1:4")
                && stderr.contains(&format!("skipped by {label}")),
            "{label}: unparsable input is reported: {stderr}"
        );
        for (name, content) in inputs {
            assert_eq!(
                fs::read_to_string(dir.path().join(name)).unwrap(),
                content,
                "{label}: {name} must be unchanged"
            );
        }
    }
}

#[test]
fn stdin_is_echoed_to_stdout_and_check_prints_nothing() {
    let dir = tempdir().unwrap();
    let cases: [(&[&str], &str, &str, &str); 4] = [
        (&[], "---\na: 1\n", "---\na: 1\n", ""),
        (&[], "\u{feff}---\na: 1\n", "\u{feff}---\na: 1\n", ""),
        (
            &["--stdin-filename", "s.yaml"],
            "a: [\n",
            "a: [\n",
            "s.yaml:1:4 skipped by ryl format",
        ),
        (&["--check"], "---\na: 1\n", "", ""),
    ];
    for (args, input, expected_stdout, expected_notice) in cases {
        let (code, stdout, stderr) = run_with_stdin(
            ryl(dir.path()).arg("format").args(args).arg("-"),
            input.as_bytes(),
        );
        assert_eq!(code, 0, "{args:?}: {stderr}");
        assert_eq!(stdout, expected_stdout, "{args:?}: stdout");
        assert!(stderr.contains(expected_notice), "{args:?}: {stderr}");
    }
}

#[test]
fn document_start_is_added_only_when_configured() {
    let dir = tempdir().unwrap();
    let add = "[format]\ndocument-start = \"add\"\n";
    for (config, expected) in [(None, "a: 1\n"), (Some(add), "---\na: 1\n")] {
        let mut cmd = ryl(dir.path());
        cmd.arg("format");
        if let Some(config) = config {
            cmd.args(["-d", config]);
        }
        let (code, stdout, stderr) = run_with_stdin(cmd.arg("-"), b"a: 1\n");
        assert_eq!(code, 0, "{config:?}: {stderr}");
        assert_eq!(stdout, expected, "{config:?}");
    }
}

#[test]
fn ignored_stdin_filename_passes_through() {
    let ignore = [
        "-d",
        "ignore: ignored.yaml",
        "--stdin-filename",
        "ignored.yaml",
    ];
    let (code, stdout, stderr) =
        run_with_stdin(exe().arg("format").args(ignore).arg("-"), b"a:   1\n");
    assert_eq!((code, stdout.as_str()), (0, "a:   1\n"), "{stderr}");
    let (code, stdout, stderr) = run_with_stdin(
        exe().args(["format", "--check"]).args(ignore).arg("-"),
        b"a:   1\n",
    );
    assert_eq!((code, stdout.as_str()), (0, ""), "{stderr}");
}

#[test]
fn unusable_inputs_are_errors() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing.yaml");
    let missing = missing.to_str().unwrap();
    let notes = dir.path().join("notes.txt");
    let notes = notes.to_str().unwrap();
    let cases: [(&[&str], &[u8]); 7] = [
        (&[], b""),
        (&["-", missing], b""),
        (&[missing], b""),
        (&["--check", missing], b""),
        (&["-c", missing, "-"], b""),
        (&["--stdin-filename", notes, "-"], b""),
        (&["-"], &[0xFF, 0xFF, 0xFF]),
    ];
    for (args, input) in cases {
        let (code, stdout, stderr) =
            run_with_stdin(ryl(dir.path()).arg("format").args(args), input);
        assert_eq!((code, stdout.as_str()), (2, ""), "{args:?}: {stderr}");
    }
}

#[test]
fn no_warnings_silences_config_deprecation_notices() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), "[rules]\n").unwrap();
    fs::write(dir.path().join("a.yaml"), "a: 1\n").unwrap();
    for (flag, warns) in [(None, true), (Some("--no-warnings"), false)] {
        let (code, _, stderr) =
            run(ryl(dir.path()).arg("format").args(flag).arg(dir.path()));
        assert_eq!(code, 0, "{flag:?}: {stderr}");
        assert_eq!(stderr.contains("deprecated"), warns, "{flag:?}: {stderr}");
    }
}
