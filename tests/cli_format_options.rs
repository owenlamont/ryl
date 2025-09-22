use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run ryl");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn disable_doc_start_config(dir: &std::path::Path) -> std::path::PathBuf {
    let cfg = dir.join("config.yml");
    fs::write(
        &cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: enable\n",
    )
    .unwrap();
    cfg
}

#[test]
fn parsable_format_outputs_expected_diagnostic() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dir.path().join("missing.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("parsable")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "parsable format should keep error exit");
    assert!(stdout.is_empty(), "parsable format should write to stderr");
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "expected single diagnostic line: {stderr}");
    let line = lines[0];
    assert!(
        line.contains(&format!(":{}:{}: [error]", 1, 11)),
        "missing location: {line}"
    );
    assert!(
        line.contains("no new line character at the end of file (new-line-at-end-of-file)"),
        "unexpected diagnostic payload: {line}"
    );

    let warn_cfg = dir.path().join("config-warning.yml");
    fs::write(
        &warn_cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file:\n    level: warning\n",
    )
    .unwrap();
    let (warn_code, warn_stdout, warn_stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("parsable")
        .arg("-c")
        .arg(&warn_cfg)
        .arg(&file));
    assert_eq!(warn_code, 0, "warning-level parsable format should exit 0");
    assert!(warn_stdout.is_empty(), "warnings should emit on stderr");
    assert!(
        warn_stderr.contains("[warning]"),
        "expected warning line: {warn_stderr}"
    );
}

#[test]
fn parsable_format_omits_rule_suffix_for_syntax_errors() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dir.path().join("invalid.yaml");
    fs::write(&file, "foo: [1, 2\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("parsable")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "syntax errors should exit 1");
    assert!(
        stdout.is_empty(),
        "syntax diagnostics should print to stderr"
    );
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "expected single diagnostic line: {stderr}");
    let diagnostic = lines[0];
    assert!(
        diagnostic.contains("[error]"),
        "syntax diagnostic must report an error: {diagnostic}"
    );
    assert!(
        diagnostic.contains("(syntax)"),
        "missing syntax marker: {diagnostic}"
    );
    assert!(
        !diagnostic.contains("(syntax) ("),
        "syntax diagnostics must not include rule suffix: {diagnostic}"
    );
}

#[test]
fn github_format_emits_workflow_commands() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dir.path().join("missing.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("github")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "github format should keep error exit");
    assert!(stdout.is_empty(), "github format writes to stderr");
    assert!(
        stderr.contains("::group::"),
        "missing GitHub group: {stderr}"
    );
    assert!(
        stderr.contains("::error file="),
        "missing GitHub error command: {stderr}"
    );
    assert!(
        stderr.contains("::endgroup::"),
        "missing GitHub endgroup: {stderr}"
    );
}

#[test]
fn colored_format_uses_ansi_sequences() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dir.path().join("missing.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("colored")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "colored format should keep error exit");
    assert!(stdout.is_empty(), "colored format writes to stderr");
    assert!(
        stderr.contains("\u{001b}[4m") && stderr.contains("\u{001b}[31m"),
        "expected ANSI sequences in colored output: {stderr}"
    );

    let warn_cfg = dir.path().join("config-warning.yml");
    fs::write(
        &warn_cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file:\n    level: warning\n",
    )
    .unwrap();
    let (warn_code, warn_stdout, warn_stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("colored")
        .arg("-c")
        .arg(&warn_cfg)
        .arg(&file));
    assert_eq!(warn_code, 0, "warning-level colored output should exit 0");
    assert!(warn_stdout.is_empty(), "warnings should emit on stderr");
    assert!(
        warn_stderr.contains("\u{001b}[33mwarning")
            && warn_stderr.contains("(new-line-at-end-of-file)"),
        "expected colored warning payload: {warn_stderr}"
    );
}

#[test]
fn colored_format_omits_rule_suffix_for_syntax_errors() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dir.path().join("syntax.yaml");
    fs::write(&file, "foo: [1, 2\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("colored")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "syntax errors should exit 1");
    assert!(
        stdout.is_empty(),
        "syntax diagnostics should print to stderr"
    );
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert!(
        lines.len() >= 2,
        "expected path and diagnostic lines: {stderr}"
    );
    let diagnostic = lines[1];
    assert!(
        diagnostic.contains("(syntax)"),
        "missing syntax marker: {diagnostic}"
    );
    assert!(
        !diagnostic.contains("  \u{001b}[2m("),
        "syntax diagnostics must not include colored rule suffix: {diagnostic}"
    );
}

#[test]
fn standard_format_remains_plain_text() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dir.path().join("missing.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("standard")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "standard format should keep error exit");
    assert!(stdout.is_empty(), "standard format writes to stderr");
    assert!(
        !stderr.contains("\u{001b}"),
        "standard format should not use ANSI: {stderr}"
    );
    assert!(
        !stderr.contains("::group::"),
        "standard format should not emit GitHub commands: {stderr}"
    );
}

#[test]
fn auto_format_honors_force_color_env() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dir.path().join("missing.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("FORCE_COLOR", "1")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_WORKFLOW")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "auto format should keep error exit");
    assert!(
        stdout.is_empty(),
        "auto format writes diagnostics to stderr"
    );
    assert!(
        stderr.contains("\u{001b}[4m") && stderr.contains("\u{001b}[31m"),
        "force color should enable colored output: {stderr}"
    );
}

#[test]
fn auto_format_respects_no_color_env() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dir.path().join("missing.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("FORCE_COLOR", "1")
        .env("NO_COLOR", "1")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_WORKFLOW")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "auto format with NO_COLOR keeps error exit");
    assert!(stdout.is_empty(), "diagnostics should be on stderr");
    assert!(
        !stderr.contains("\u{001b}"),
        "NO_COLOR should disable ANSI sequences: {stderr}"
    );
}
