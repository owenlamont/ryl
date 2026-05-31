//! End-to-end directive coverage through the binary: stdin and embedded-markdown
//! (check + `--fix`). Engine semantics live in `directives.rs`; this guards the wiring.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, format!("{stdout}{stderr}"))
}

fn run_stdin(input: &str, args: &[&str]) -> (i32, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(args)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (out.status.code().unwrap_or(-1), format!("{stdout}{stderr}"))
}

#[test]
fn stdin_honours_disable_line_directive() {
    let cfg = "rules: {colons: enable}";
    let (code, out) =
        run_stdin("a:  1  # ryl disable-line rule:colons\n", &["-d", cfg]);
    assert_eq!(code, 0, "directive should suppress colons: {out}");
}

#[test]
fn stdin_still_reports_without_a_matching_directive() {
    let cfg = "rules: {colons: enable}";
    let (code, out) =
        run_stdin("a:  1  # ryl disable-line rule:commas\n", &["-d", cfg]);
    assert_eq!(code, 1, "unrelated directive must not suppress colons");
    assert!(out.contains("1:4"), "expected colons at 1:4: {out}");
    assert!(out.contains("colons"), "expected colons rule id: {out}");
}

fn project(
    config: &str,
    name: &str,
    body: &str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    let file = dir.path().join(name);
    fs::write(&file, body.as_bytes()).unwrap();
    (dir, file)
}

const MD_COLONS: &str =
    "files = { markdown = [\"*.md\"] }\n[rules]\ncolons = \"enable\"\n";
const MD_COMMAS: &str =
    "files = { markdown = [\"*.md\"] }\n[rules]\ncommas = \"enable\"\n";

#[test]
fn markdown_fenced_block_honours_directive() {
    let body = "# doc\n\n```yaml\na:  1  # ryl disable-line rule:colons\nb:  2\n```\n";
    let (_dir, file) = project(MD_COLONS, "doc.md", body);

    let (code, out) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));
    assert_eq!(code, 1, "the non-disabled line should still fail: {out}");
    // The fenced block starts on markdown line 3, so the disabled YAML line is line 4
    // and `b:  2` is line 5.
    assert!(out.contains("5:4"), "expected colons on line 5: {out}");
    assert!(
        !out.contains("4:4"),
        "line 4 directive must suppress colons: {out}"
    );
}

#[test]
fn markdown_fix_skips_directive_disabled_line() {
    let body =
        "# doc\n\n```yaml\na: [1,2]  # ryl disable-line rule:commas\nb: [3,4]\n```\n";
    let (_dir, file) = project(MD_COMMAS, "doc.md", body);

    let (code, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--fix")
        .arg(&file));
    assert_eq!(code, 0, "fix should succeed: {err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# doc\n\n```yaml\na: [1,2]  # ryl disable-line rule:commas\nb: [3, 4]\n```\n",
        "disabled line untouched; the other line is fixed"
    );
}
