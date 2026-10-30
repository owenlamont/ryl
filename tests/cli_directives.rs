//! End-to-end directive coverage through the binary: stdin and embedded-markdown
//! (check + `--fix`). Engine semantics live in `directives.rs`; this guards the wiring.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

use crate::common;

fn run(cmd: &mut Command) -> (i32, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy_owned(out.stdout);
    let stderr = String::from_utf8_lossy_owned(out.stderr);
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
    let stdout = String::from_utf8_lossy_owned(out.stdout);
    let stderr = String::from_utf8_lossy_owned(out.stderr);
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
    "files = { markdown = [\"*.md\"] }\n[lint.rules]\ncolons = \"enable\"\n";
const MD_COMMAS: &str =
    "files = { markdown = [\"*.md\"] }\n[lint.rules]\ncommas = \"enable\"\n";

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

#[test]
fn stdin_honours_directive_beside_a_version_comment() {
    let uses = r#""uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1""#;
    for comment in [
        "# v1.2.3  # ryl disable-line rule:line-length",
        "# ryl disable-line rule:line-length  # v1.2.3",
    ] {
        let input = format!("---\n{uses}  {comment}\n");
        let (code, out) = run_stdin(&input, &["--config-data", "{extends: default}"]);
        assert_eq!(code, 0, "{comment:?} should suppress line-length: {out}");
    }
}

const TEMPLATE: &str = "#!jinja|yaml\n{% set v = 1 %}\na:  \"{{ v }}\"\n";

#[test]
fn discovered_jinja_template_is_skipped() {
    let td = tempdir().unwrap();
    fs::write(
        td.path().join(".ryl.toml"),
        "[files]\nmarkdown = [\"*.md\"]\n[lint.rules]\ncolons = \"enable\"\n",
    )
    .unwrap();
    fs::write(td.path().join("t.yaml"), TEMPLATE).unwrap();
    fs::write(td.path().join("b.yaml"), format!("\u{feff}{TEMPLATE}")).unwrap();
    fs::write(
        td.path().join("doc.md"),
        format!("```yaml\n{TEMPLATE}```\n"),
    )
    .unwrap();
    fs::write(td.path().join("ok.yaml"), "a: 1\n").unwrap();
    let ryl = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ryl"));
        cmd.current_dir(td.path());
        cmd
    };
    let (code, out) = run(ryl().args(["check", "--list-files", "."]));
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("ok.yaml") && !out.contains("t.yaml") && !out.contains("b.yaml"),
        "template not listed: {out}"
    );
    for args in [
        &["check", "."][..],
        &["check", "--diff", "."],
        &["format", "--diff", "."],
    ] {
        let (code, out) = run(ryl().args(args));
        assert_eq!(code, 0, "{args:?} skips the template: {out}");
        assert!(
            ["t.yaml", "b.yaml", "doc.md"]
                .iter()
                .all(|name| !out.contains(name)),
            "{args:?} skips the template: {out}"
        );
    }
}

#[test]
fn named_jinja_template_is_refused() {
    let td = tempdir().unwrap();
    let path = td.path().join("t.yaml");
    let header = format!("#!yaml{}|jinja\n", " ".repeat(300));
    fs::write(&path, common::encoding::encoded(&header, 2, true, true)).unwrap();
    for args in [
        &["check", "-d", "[lint.rules]\ncolons = \"enable\""][..],
        &["format"],
    ] {
        let (code, out) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
            .env("HOME", td.path())
            .args(args)
            .arg(&path));
        assert_eq!(code, 2, "{args:?} refuses a named template: {out}");
        assert!(out.contains("a Jinja template"), "{args:?}: {out}");
    }
    for args in [
        &["check", "-d", "[lint.rules]\ncolons = \"enable\""][..],
        &["format", "--diff"],
        &["format"],
    ] {
        let (code, out) = run_stdin(TEMPLATE, args);
        assert_eq!(code, 2, "{args:?} refuses a template on stdin: {out}");
        assert!(out.contains("<stdin>: a Jinja template"), "{args:?}: {out}");
    }
}
