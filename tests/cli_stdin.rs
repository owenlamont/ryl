use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

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

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn stdin_with_no_enabled_rules_errors() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe).arg("-").arg("-d").arg("rules: {}\n"),
        b"key: value\n",
    );
    assert_eq!(
        code, 2,
        "stdin lint with no enabled rules must error: {stderr}"
    );
    assert!(stderr.contains("enables no rules"), "{stderr}");
}

#[test]
fn stdin_clean_yaml_succeeds_and_uses_label() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run_with_stdin(Command::new(exe).arg("-"), b"key: value\n");
    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
    assert!(stdout.is_empty(), "expected empty stdout: {stdout}");
    assert!(stderr.is_empty(), "expected empty stderr: {stderr}");
}

#[test]
fn stdin_with_diagnostics_reports_stdin_label() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) =
        run_with_stdin(Command::new(exe).arg("-"), b"key:  value\n");
    assert_eq!(code, 0, "warnings should not fail: stderr={stderr}");
    assert!(stderr.contains("<stdin>"), "expected stdin label: {stderr}");
    assert!(
        stderr.contains("too many spaces after colon"),
        "expected colons message: {stderr}"
    );
}

#[test]
fn stdin_missing_newline_reports_error() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) =
        run_with_stdin(Command::new(exe).arg("-"), b"key: value");
    assert_eq!(code, 1, "expected failure: stderr={stderr}");
    assert!(
        stderr.contains("no new line character at the end of file"),
        "expected new-line rule message: {stderr}"
    );
}

#[test]
fn stdin_syntax_error_reports_failure() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) =
        run_with_stdin(Command::new(exe).arg("-"), b"key: [unterminated\n");
    assert_eq!(code, 1, "expected failure: stderr={stderr}");
    assert!(
        stderr.contains("syntax error"),
        "expected syntax error: {stderr}"
    );
    assert!(stderr.contains("<stdin>"), "expected stdin label: {stderr}");
}

#[test]
fn stdin_filename_appears_in_diagnostics() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("-")
            .arg("--stdin-filename")
            .arg("pkg/app.yaml"),
        b"key:  value\n",
    );
    assert_eq!(code, 0, "warnings should not fail: {stderr}");
    assert!(
        stderr.contains("pkg/app.yaml"),
        "expected provided filename label: {stderr}"
    );
    assert!(
        !stderr.contains("<stdin>"),
        "stdin label should be replaced: {stderr}"
    );
}

#[test]
fn stdin_filename_no_source_kind_errors() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("-")
            .arg("--stdin-filename")
            .arg("script.sh"),
        b"key:  value\n",
    );
    assert_eq!(
        code, 2,
        "unmatched stdin filename errors like an explicit file: {stderr}"
    );
    assert!(stdout.is_empty(), "stdout should be empty: {stdout}");
    assert!(
        stderr.contains("no source kind matches"),
        "expected no-source-kind error: {stderr}"
    );
}

#[test]
fn stdin_without_filename_runs_rules_for_non_yaml_extensions() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) =
        run_with_stdin(Command::new(exe).arg("-"), b"key:  value\n");
    assert_eq!(code, 0, "warnings should not fail: {stderr}");
    assert!(
        stderr.contains("too many spaces after colon"),
        "yaml-files filtering should be skipped without stdin-filename: {stderr}"
    );
}

#[test]
fn stdin_filename_anchors_project_config_discovery() {
    let dir = tempdir().unwrap();
    let pkg = dir.path().join("pkg");
    fs::create_dir(&pkg).unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\nkey-duplicates = \"disable\"\ntrailing-spaces = \"enable\"\n",
    )
    .unwrap();

    let stdin_path = pkg.join("app.yaml");
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .current_dir(&pkg)
            .arg("-")
            .arg("--stdin-filename")
            .arg(&stdin_path),
        b"key: one\nkey: two\n",
    );
    assert_eq!(
        code, 0,
        "rule should be disabled by parent config: stdout={stdout} stderr={stderr}"
    );
}

#[test]
fn stdin_without_filename_ignores_per_file_ignores() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = \"enable\"\n\n[per-file-ignores]\n\"*\" = [\"document-start\"]\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe).current_dir(dir.path()).arg("-"),
        b"name: value\n",
    );
    assert_eq!(
        code, 1,
        "per-file-ignores must not match synthetic stdin label: {stderr}"
    );
    assert!(
        stderr.contains("document-start"),
        "expected document-start to still fire: {stderr}"
    );
}

#[test]
fn stdin_without_filename_ignores_rule_level_ignore_patterns() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".yamllint"),
        "rules:\n  document-start:\n    level: error\n    ignore: |\n      *\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe).current_dir(dir.path()).arg("-"),
        b"name: value\n",
    );
    assert_eq!(
        code, 1,
        "rule-level ignore must not match synthetic stdin label: {stderr}"
    );
    assert!(
        stderr.contains("document-start"),
        "expected document-start to still fire: {stderr}"
    );
}

#[test]
fn stdin_filename_respects_per_file_ignores() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = \"enable\"\n\n[per-file-ignores]\n\"ignored.yaml\" = [\"document-start\"]\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .current_dir(dir.path())
            .arg("-")
            .arg("--stdin-filename")
            .arg("ignored.yaml"),
        b"name: value\n",
    );
    assert_eq!(
        code, 0,
        "per-file-ignore should silence document-start: {stderr}"
    );
}

#[test]
fn stdin_combined_with_other_input_errors() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe).arg("-").arg("file.yaml"));
    assert_eq!(code, 2, "expected usage error: {stderr}");
    assert!(
        stderr.contains("cannot be combined"),
        "expected combined-inputs error: {stderr}"
    );
}

#[test]
fn stdin_filename_without_dash_errors() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) =
        run(Command::new(exe).arg("--stdin-filename").arg("foo.yaml"));
    assert_eq!(code, 2, "expected usage error: {stderr}");
    assert!(
        stderr.contains("only applies when reading from stdin"),
        "expected stdin-filename error: {stderr}"
    );
}

#[test]
fn fix_with_stdin_errors() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) =
        run_with_stdin(Command::new(exe).arg("--fix").arg("-"), b"key: value\n");
    assert_eq!(code, 2, "expected usage error: {stderr}");
    assert!(stderr.contains("--fix"), "expected --fix error: {stderr}");
}

#[test]
fn stdin_list_files_prints_label() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run_with_stdin(
        Command::new(exe).arg("--list-files").arg("-"),
        b"key: value\n",
    );
    assert_eq!(code, 0, "expected success: {stderr}");
    assert!(
        stdout.contains("<stdin>"),
        "expected stdin label in stdout: {stdout}"
    );
}

#[test]
fn stdin_list_files_uses_stdin_filename() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("--list-files")
            .arg("-")
            .arg("--stdin-filename")
            .arg("foo.yaml"),
        b"key: value\n",
    );
    assert_eq!(code, 0, "expected success: {stderr}");
    assert!(stdout.contains("foo.yaml"), "expected filename: {stdout}");
}

#[test]
fn stdin_honors_config_data_override() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe).arg("-").arg("-d").arg(
            "rules:\n  new-line-at-end-of-file: disable\n  key-duplicates: enable\n",
        ),
        b"key: value",
    );
    assert_eq!(code, 0, "disabled rule should pass: {stderr}");
}

#[test]
fn stdin_decodes_utf16_bom() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "key: value\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let (code, stdout, stderr) = run_with_stdin(Command::new(exe).arg("-"), &bytes);
    assert_eq!(
        code, 0,
        "utf-16 stdin should decode cleanly: stdout={stdout} stderr={stderr}"
    );
}

#[test]
fn stdin_strict_mode_treats_warnings_as_failure() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) =
        run_with_stdin(Command::new(exe).arg("--strict").arg("-"), b"key:  value\n");
    assert_eq!(code, 2, "strict mode should exit 2: {stderr}");
}

#[test]
fn stdin_invalid_utf8_reports_decode_error() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) =
        run_with_stdin(Command::new(exe).arg("-"), &[0xFF, 0xFF, 0xFF]);
    assert_eq!(code, 1, "expected lint failure: {stderr}");
    assert!(
        stderr.contains("failed to read <stdin>"),
        "expected decode error referencing stdin label: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn stdin_read_error_reports_lint_failure() {
    let dir = tempdir().unwrap();
    let dir_fd = std::fs::File::open(dir.path()).expect("open directory fd");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let out = Command::new(exe)
        .arg("-")
        .stdin(Stdio::from(dir_fd))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn ryl");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(code, 1, "expected lint failure: {stderr}");
    assert!(
        stderr.contains("failed to read <stdin>"),
        "expected stdin read error: {stderr}"
    );
}

#[test]
fn stdin_filename_decode_error_uses_filename_in_message() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("-")
            .arg("--stdin-filename")
            .arg("buffer.yaml"),
        &[0xFF, 0xFF, 0xFF],
    );
    assert_eq!(code, 1, "expected lint failure: {stderr}");
    assert!(
        stderr.contains("failed to read buffer.yaml"),
        "expected decode error to reference stdin-filename: {stderr}"
    );
}

#[test]
fn stdin_with_missing_config_file_errors() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("-")
            .arg("-c")
            .arg("/nonexistent/path/to/config.yml"),
        b"key: value\n",
    );
    assert_eq!(code, 2, "expected usage error: {stderr}");
}

#[test]
fn stdin_emits_legacy_yaml_notice_when_toml_present() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ntrailing-spaces = \"enable\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join(".yamllint"),
        "rules:\n  trailing-spaces: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe).current_dir(dir.path()).arg("-"),
        b"key: value\n",
    );
    assert_eq!(code, 0, "expected success: {stderr}");
    assert!(
        stderr.contains("ignoring legacy YAML config discovery"),
        "expected TOML-over-YAML notice: {stderr}"
    );
}

#[test]
fn stdin_no_warnings_suppresses_warning_output() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run_with_stdin(
        Command::new(exe).arg("--no-warnings").arg("-"),
        b"key:  value\n",
    );
    assert_eq!(code, 0, "warnings should be suppressed: {stderr}");
    assert!(stdout.trim().is_empty(), "expected empty stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected empty stderr: {stderr}");
}

#[test]
fn stdin_markdown_filename_lints_embedded_yaml() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "files = { markdown = [\"*.md\"] }\n[rules]\ncolons = \"enable\"\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .current_dir(dir.path())
            .arg("-")
            .arg("--stdin-filename")
            .arg(dir.path().join("doc.md")),
        b"---\nfoo:  bar\n---\n",
    );
    assert_eq!(code, 1, "embedded YAML must be linted: {stderr}");
    assert!(
        stderr.contains("2:6") && stderr.contains("colons"),
        "front-matter diagnostic maps to host position: {stderr}"
    );
}

#[test]
fn stdin_markdown_flag_lints_embedded_yaml() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("--markdown")
            .arg("-")
            .arg("-d")
            .arg("rules: {colons: enable}"),
        b"# t\n\n```yaml\nfoo:  bar\n```\n",
    );
    assert_eq!(code, 1, "fenced block must be linted: {stderr}");
    assert!(
        stderr.contains("<stdin>") && stderr.contains("4:6"),
        "fenced diagnostic uses stdin label and host position: {stderr}"
    );
}

#[test]
fn stdin_markdown_filename_ignored_is_skipped() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "files = { markdown = [\"*.md\"] }\nignore = [\"doc.md\"]\n[rules]\ncolons = \"enable\"\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .current_dir(dir.path())
            .arg("-")
            .arg("--stdin-filename")
            .arg(dir.path().join("doc.md")),
        b"---\nfoo:  bar\n---\n",
    );
    assert_eq!(code, 0, "ignored stdin path must be skipped: {stderr}");
    assert!(
        stdout.is_empty() && stderr.is_empty(),
        "out={stdout} err={stderr}"
    );
}

#[test]
fn stdin_markdown_flag_overrides_non_markdown_filename() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .arg("--markdown")
            .arg("-")
            .arg("--stdin-filename")
            .arg("notes.txt")
            .arg("-d")
            .arg("rules: {colons: enable}"),
        b"---\nfoo:  bar\n---\n",
    );
    assert_eq!(
        code, 1,
        "--markdown must force markdown regardless of filename: {stderr}"
    );
    assert!(
        stderr.contains("notes.txt")
            && stderr.contains("2:6")
            && stderr.contains("colons"),
        "embedded front matter linted under the provided label: {stderr}"
    );
}

#[test]
fn stdin_markdown_overlap_is_a_hard_error() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "files = { yaml = [\"*.md\"], markdown = [\"*.md\"] }\n[rules]\ncolons = \"enable\"\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run_with_stdin(
        Command::new(exe)
            .current_dir(dir.path())
            .arg("-")
            .arg("--stdin-filename")
            .arg(dir.path().join("doc.md")),
        b"foo: bar\n",
    );
    assert_eq!(code, 2, "overlap must be a usage error: {stderr}");
    assert!(stderr.contains("matches both"), "{stderr}");
}
