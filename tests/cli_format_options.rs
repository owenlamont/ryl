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
        line.contains(
            "no new line character at the end of file (new-line-at-end-of-file)"
        ),
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
fn github_format_escapes_newlines_to_prevent_command_injection() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yml");
    fs::write(&cfg, "rules:\n  key-duplicates: enable\n").unwrap();
    let file = dir.path().join("inject.yaml");
    // A duplicate key whose name embeds a newline and a fake workflow command;
    // the key text is echoed verbatim into the key-duplicates message.
    fs::write(
        &file,
        "\"x\\n::error::INJECTED\": 1\n\"x\\n::error::INJECTED\": 2\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("github")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert!(
        stderr.contains("%0A"),
        "the newline in the key must be encoded as %0A: {stderr}"
    );
    assert!(
        !stderr.contains("\n::error::INJECTED"),
        "an embedded newline must not start a new ::error:: command: {stderr}"
    );
}

#[test]
fn human_formats_escape_control_characters_in_messages() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yml");
    fs::write(&cfg, "rules:\n  key-duplicates: enable\n").unwrap();
    let file = dir.path().join("esc.yaml");
    // A duplicate key carrying a raw ESC (YAML \x1b) — echoed into the message; the
    // parsable format has no ANSI of its own, so any ESC in the output is the payload.
    fs::write(&file, "\"\\x1b[2Jx\": 1\n\"\\x1b[2Jx\": 2\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("parsable")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert!(
        !stderr.contains('\u{1b}'),
        "a raw ESC control char must not reach the terminal: {stderr:?}"
    );
    assert!(
        stderr.contains("\\u{1b}"),
        "the control char must be rendered as a visible escape: {stderr}"
    );
}

#[test]
fn github_format_escapes_control_chars_not_just_newlines() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yml");
    fs::write(&cfg, "rules:\n  key-duplicates: enable\n").unwrap();
    let file = dir.path().join("esc.yaml");
    fs::write(&file, "\"\\x1b[31mEVIL\": 1\n\"\\x1b[31mEVIL\": 2\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("github")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    // The GitHub format has no ANSI of its own, so a raw ESC would be the payload
    // reaching the CI log viewer; it must be rendered as a literal escape, not a
    // %XX (which the runner would decode back into a control char).
    assert!(
        !stderr.contains('\u{1b}'),
        "raw ESC must not reach the GitHub log: {stderr:?}"
    );
    assert!(
        stderr.contains("\\u{1b}"),
        "ESC must be rendered as a literal escape in the GitHub format: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn github_format_escapes_percent_cr_and_property_delimiters() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yml");
    fs::write(&cfg, "rules:\n  key-duplicates: enable\n").unwrap();
    // The filename's `:`/`,` exercise the `file=` property escaping; the duplicate
    // key carries a literal `%` and a CR into the message.
    let file = dir.path().join("we:ird,name.yaml");
    fs::write(&file, "\"a%b\\rc\": 1\n\"a%b\\rc\": 2\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("github")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert!(
        stderr.contains("%3A") && stderr.contains("%2C"),
        "`:` and `,` in the file= path must be percent-escaped: {stderr}"
    );
    assert!(
        stderr.contains("%25"),
        "a literal `%` must become %25: {stderr}"
    );
    assert!(stderr.contains("%0D"), "a CR must become %0D: {stderr}");
}

#[cfg(unix)]
#[test]
fn error_message_paths_are_escaped_in_github_format() {
    // A filename with an embedded newline + a fake workflow command, whose content
    // is invalid UTF-8 so linting fails and the error message (which embeds the
    // path) is emitted. The path must be sanitized so the newline cannot start a
    // new ::command:: in CI.
    let dir = tempdir().unwrap();
    let file = dir.path().join("evil\n::error::ARM_INJECT.yaml");
    fs::write(&file, [0x80u8]).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("github")
        .arg("-d")
        .arg("rules:\n  document-start: enable\n")
        .arg(&file));
    assert!(
        !stderr.contains("\n::error::ARM_INJECT"),
        "an error-message path must not inject a workflow command: {stderr:?}"
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
fn colored_format_matches_reference_layout() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("layout.yaml");
    fs::write(&file, "list: [1,2]\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("colored")
        .arg("-d")
        .arg("extends: default")
        .arg(&file));
    assert_eq!(code, 1, "colored format should exit 1 when errors occur");
    assert!(
        stdout.is_empty(),
        "colored format diagnostics must print on stderr"
    );
    let expected = format!(
        "\u{001b}[4m{path}\u{001b}[0m\n  \u{001b}[2m1:1\u{001b}[0m       \u{001b}[33mwarning\u{001b}[0m  missing document start \"---\"  \u{001b}[2m(document-start)\u{001b}[0m\n  \u{001b}[2m1:10\u{001b}[0m      \u{001b}[31merror\u{001b}[0m    too few spaces after comma  \u{001b}[2m(commas)\u{001b}[0m\n\n",
        path = file.display()
    );
    assert_eq!(stderr, expected, "colored diagnostic payload mismatch");
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
        .env_remove("NO_COLOR")
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

// --- JUnit / GitLab report formats (issue #285) ---

/// A file missing its trailing newline trips `new-line-at-end-of-file`, giving every
/// report format at least one diagnostic to render.
fn dirty_yaml(dir: &std::path::Path) -> std::path::PathBuf {
    let file = dir.join("dirty.yaml");
    fs::write(&file, "key: value").unwrap();
    file
}

#[test]
fn gitlab_format_writes_json_array_to_stdout() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dirty_yaml(dir.path());

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("gitlab")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "gitlab format keeps the error exit code");
    assert!(
        stderr.is_empty(),
        "report formats go to stdout, not stderr: {stderr}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("gitlab output is a JSON array");
    let issues = json.as_array().expect("top level array");
    assert_eq!(issues.len(), 1, "one diagnostic expected: {stdout}");
    assert_eq!(issues[0]["check_name"], "new-line-at-end-of-file");
    assert_eq!(issues[0]["severity"], "major");
    assert_eq!(issues[0]["location"]["lines"]["begin"], 1);
}

#[test]
fn junit_format_writes_xml_to_stdout() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dirty_yaml(dir.path());

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("junit")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "junit format keeps the error exit code");
    assert!(
        stderr.is_empty(),
        "report formats go to stdout, not stderr: {stderr}"
    );
    assert!(
        stdout.contains("<testsuites name=\"ryl\""),
        "junit output should start with a testsuites root: {stdout}"
    );
    assert!(
        stdout.contains("type=\"new-line-at-end-of-file\""),
        "the diagnostic's rule id should appear as the failure type: {stdout}"
    );
}

#[test]
fn output_file_writes_report_and_leaves_streams_clean() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dirty_yaml(dir.path());
    let report = dir.path().join("report.xml");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("junit")
        .arg("-o")
        .arg(&report)
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "--output-file keeps the error exit code");
    assert!(
        stdout.is_empty(),
        "with --output-file nothing goes to stdout: {stdout}"
    );
    assert!(
        stderr.is_empty(),
        "with --output-file nothing goes to stderr: {stderr}"
    );
    let written = fs::read_to_string(&report).expect("report file written");
    assert!(
        written.contains("<testsuites"),
        "the report file holds the junit document: {written}"
    );
}

#[test]
fn output_file_redirects_streaming_format() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dirty_yaml(dir.path());
    let report = dir.path().join("out.txt");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("parsable")
        .arg("-o")
        .arg(&report)
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1, "redirected streaming format keeps the error exit");
    assert!(
        stderr.is_empty(),
        "--output-file diverts the streaming diagnostics: {stderr}"
    );
    let written = fs::read_to_string(&report).expect("report file written");
    assert!(
        written.contains(": [error]") && written.contains("new-line-at-end-of-file"),
        "the parsable diagnostic should be in the file: {written}"
    );
}

#[test]
fn output_file_open_failure_is_usage_error() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dirty_yaml(dir.path());
    // A path under a directory that does not exist cannot be created.
    let report = dir.path().join("missing-dir").join("report.json");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("gitlab")
        .arg("-o")
        .arg(&report)
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 2, "an unopenable --output-file is a usage error");
    assert!(
        stderr.contains("cannot open --output-file"),
        "expected an open-failure message: {stderr}"
    );
}

#[test]
fn diff_with_report_format_is_usage_error() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dirty_yaml(dir.path());

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("gitlab")
        .arg("--diff")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 2, "--diff with a report format is a usage error");
    assert!(
        stderr.contains("`--diff` cannot be combined with"),
        "expected the diff/report conflict message: {stderr}"
    );
}

#[test]
fn gitlab_reports_processing_error_as_blocker() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let missing = dir.path().join("absent.yaml");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, _stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("gitlab")
        .arg("-c")
        .arg(&cfg)
        .arg(&missing));
    assert_eq!(code, 1, "a file that cannot be read is an error");
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("gitlab output is JSON");
    let issues = json.as_array().expect("array");
    assert_eq!(issues.len(), 1, "the read failure is one issue: {stdout}");
    assert_eq!(issues[0]["check_name"], "error");
    assert_eq!(issues[0]["severity"], "blocker");
}

#[test]
fn gitlab_format_reads_stdin_with_filename() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());

    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut child = Command::new(exe)
        // The path is relativized against CI_PROJECT_DIR when set; clear it so the
        // assertion holds regardless of the surrounding (GitLab) CI environment.
        .env_remove("CI_PROJECT_DIR")
        .arg("-")
        .arg("--format")
        .arg("gitlab")
        .arg("--stdin-filename")
        .arg("nested/from-stdin.yaml")
        .arg("-c")
        .arg(&cfg)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ryl");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"key: value")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdin diagnostic keeps error exit"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("gitlab output is JSON");
    assert_eq!(
        json[0]["location"]["path"], "nested/from-stdin.yaml",
        "the stdin filename becomes location.path: {stdout}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn output_file_write_failure_is_reported() {
    // `/dev/full` opens successfully but fails every write with ENOSPC, exercising the
    // destination-write error path (the one fallible step of the output pipeline).
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dirty_yaml(dir.path());

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("gitlab")
        .arg("-o")
        .arg("/dev/full")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 2, "a failed write is a usage error");
    assert!(
        stderr.contains("failed to write output"),
        "expected a write-failure message: {stderr}"
    );
}

/// Spawn ryl reading `input` from stdin with the given args, returning (code, stdout, stderr).
fn run_stdin(args: &[&str], input: &[u8]) -> (i32, String, String) {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut child = Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ryl");
    use std::io::Write as _;
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn stdin_diff_with_report_format_is_usage_error() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let cfg = cfg.to_str().unwrap();

    let (code, _stdout, stderr) = run_stdin(
        &["-", "--format", "gitlab", "--diff", "-c", cfg],
        b"key: value",
    );
    assert_eq!(
        code, 2,
        "--diff with a report format is a usage error on stdin"
    );
    assert!(
        stderr.contains("`--diff` cannot be combined with"),
        "expected the diff/report conflict message: {stderr}"
    );
}

#[test]
fn stdin_output_file_open_failure_is_usage_error() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let bad = dir.path().join("missing-dir").join("report.json");
    let (code, _stdout, stderr) = run_stdin(
        &[
            "-",
            "--format",
            "gitlab",
            "-o",
            bad.to_str().unwrap(),
            "-c",
            cfg.to_str().unwrap(),
        ],
        b"key: value",
    );
    assert_eq!(
        code, 2,
        "an unopenable --output-file is a usage error on stdin"
    );
    assert!(
        stderr.contains("cannot open --output-file"),
        "expected an open-failure message: {stderr}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn stdin_output_file_write_failure_is_reported() {
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let (code, _stdout, stderr) = run_stdin(
        &[
            "-",
            "--format",
            "gitlab",
            "-o",
            "/dev/full",
            "-c",
            cfg.to_str().unwrap(),
        ],
        b"key: value",
    );
    assert_eq!(code, 2, "a failed write is a usage error on stdin");
    assert!(
        stderr.contains("failed to write output"),
        "expected a write-failure message: {stderr}"
    );
}

#[test]
fn output_file_pointing_at_a_linted_input_is_rejected() {
    // Guard against data loss: writing the report to a file that is also being linted
    // would truncate the source. Must be refused before any lint/fix runs.
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let file = dirty_yaml(dir.path());
    let original = fs::read_to_string(&file).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("gitlab")
        .arg("-o")
        .arg(&file)
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(
        code, 2,
        "output file colliding with an input is a usage error"
    );
    assert!(
        stderr.contains("is also a linted input"),
        "expected a collision message: {stderr}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original,
        "the input file must be left untouched"
    );
}

#[test]
fn empty_input_emits_an_empty_gitlab_report() {
    // A clean/empty project must still produce a valid `[]` artifact, not a missing file.
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let report = dir.path().join("gl.json");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, _stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("gitlab")
        .arg("-o")
        .arg(&report)
        .arg("-c")
        .arg(&cfg)
        .arg(&empty));
    assert_eq!(code, 0, "an empty project lints clean");
    assert_eq!(
        fs::read_to_string(&report).unwrap().trim(),
        "[]",
        "the report file holds an empty JSON array"
    );
}

#[test]
fn gitlab_path_is_relative_to_ci_project_dir() {
    // Like ruff, location.path is relativized against CI_PROJECT_DIR when set.
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let nested = dir.path().join("pkg");
    fs::create_dir(&nested).unwrap();
    let file = nested.join("dirty.yaml");
    fs::write(&file, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, _stderr) = run(Command::new(exe)
        .env("CI_PROJECT_DIR", dir.path())
        .arg("--format")
        .arg("gitlab")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 1);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("gitlab output is JSON");
    assert_eq!(
        json[0]["location"]["path"], "pkg/dirty.yaml",
        "path is relative to CI_PROJECT_DIR: {stdout}"
    );
}

#[test]
fn empty_input_report_open_failure_is_usage_error() {
    // The empty-report path must still surface an unopenable --output-file as a usage error.
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let report = dir.path().join("missing-dir").join("gl.json");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("gitlab")
        .arg("-o")
        .arg(&report)
        .arg("-c")
        .arg(&cfg)
        .arg(&empty));
    assert_eq!(
        code, 2,
        "an unopenable --output-file is a usage error even when empty"
    );
    assert!(
        stderr.contains("cannot open --output-file"),
        "expected an open-failure message: {stderr}"
    );
}

#[test]
fn ignored_stdin_emits_an_empty_gitlab_report() {
    // An ignored stdin filename is an empty input set; the report must still be a valid `[]`.
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "ignore = [\"ignored.yaml\"]\n[rules]\ncolons = \"enable\"\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut child = Command::new(exe)
        .current_dir(dir.path())
        .arg("-")
        .arg("--stdin-filename")
        .arg(dir.path().join("ignored.yaml"))
        .arg("--format")
        .arg("gitlab")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ryl");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"key:  value\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "ignored stdin lints clean");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "[]",
        "ignored stdin still emits an empty JSON array"
    );
}

#[test]
fn ignored_stdin_report_open_failure_is_usage_error() {
    // The ignored-stdin empty-report path must still surface an unopenable --output-file.
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "ignore = [\"ignored.yaml\"]\n[rules]\ncolons = \"enable\"\n",
    )
    .unwrap();
    let report = dir.path().join("missing-dir").join("gl.json");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut child = Command::new(exe)
        .current_dir(dir.path())
        .arg("-")
        .arg("--stdin-filename")
        .arg(dir.path().join("ignored.yaml"))
        .arg("--format")
        .arg("gitlab")
        .arg("-o")
        .arg(&report)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ryl");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"key:  value\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "unopenable --output-file is a usage error"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cannot open --output-file"),
        "expected an open-failure message: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn empty_input_with_output_file_creates_file_for_streaming_format() {
    // --output-file uniformly produces the file even for an empty streaming-format run.
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let report = dir.path().join("lint.txt");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, _stderr) = run(Command::new(exe)
        .arg("--format")
        .arg("parsable")
        .arg("-o")
        .arg(&report)
        .arg("-c")
        .arg(&cfg)
        .arg(&empty));
    assert_eq!(code, 0, "an empty project lints clean");
    assert_eq!(
        fs::read_to_string(&report).unwrap(),
        "",
        "the output file is created empty for a clean streaming run"
    );
}

#[test]
fn stdin_output_file_matching_stdin_filename_is_rejected() {
    // The stdin path must honor the same data-loss guard: `-o` equal to the
    // --stdin-filename would truncate the file that label names.
    let dir = tempdir().unwrap();
    let cfg = disable_doc_start_config(dir.path());
    let target = dir.path().join("config.yaml");
    fs::write(&target, "original: content\n").unwrap();
    let original = fs::read_to_string(&target).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut child = Command::new(exe)
        .arg("-")
        .arg("--stdin-filename")
        .arg(&target)
        .arg("--format")
        .arg("gitlab")
        .arg("-o")
        .arg(&target)
        .arg("-c")
        .arg(&cfg)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ryl");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"key: value")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdin output-file collision is a usage error"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("is also a linted input"),
        "expected a collision message: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        original,
        "the named file must be left untouched"
    );
}
