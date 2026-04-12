use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn fix_applies_safe_newline_and_comment_fixes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ncomments = 'enable'\nnew-lines = 'enable'\nnew-line-at-end-of-file = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));
    assert_eq!(
        code, 0,
        "fix should succeed: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 3 problems (3 fixed, 0 remaining)."),
        "expected fix summary in stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: value  # comment\n");
}

#[test]
fn fix_respects_toml_unfixable_rules() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ncomments = 'enable'\nnew-lines = 'enable'\nnew-line-at-end-of-file = 'enable'\n[fix]\nunfixable = ['comments']\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));
    assert_eq!(
        code, 1,
        "comment diagnostics should remain: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 3 problems (1 fixed, 2 remaining)."),
        "expected partial fix summary in stderr: {stderr}"
    );

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: value #comment\n");
}

#[test]
fn fix_respects_toml_fixable_allowlist() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ncomments = 'enable'\nnew-lines = 'enable'\nnew-line-at-end-of-file = 'enable'\n[fix]\nfixable = ['comments']\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));
    assert_eq!(
        code, 1,
        "missing final newline should remain: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 3 problems (2 fixed, 1 remaining)."),
        "expected partial fix summary in stderr: {stderr}"
    );

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: value  # comment");
}

#[test]
#[allow(clippy::permissions_set_readonly_false)]
fn fix_handles_write_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("read_only.yaml");
    // Missing newline so it needs a fix
    fs::write(&file, "key: value").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nnew-line-at-end-of-file = 'enable'\n",
    )
    .unwrap();

    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    // Reset permissions so tempdir can be cleaned up
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(false);
    let _ = fs::set_permissions(&file, perms);

    assert_eq!(
        code, 2,
        "fix should fail on read-only file: stderr={stderr}"
    );
    assert!(
        stderr.contains("failed to write fixed file"),
        "error message should mention write failure: {stderr}"
    );
}

#[test]
fn fix_reports_summary_for_invalid_yaml() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("invalid.yaml");
    fs::write(&file, "key: [\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    assert_eq!(
        code, 1,
        "invalid yaml should fail: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 1 problem (0 fixed, 1 remaining)."),
        "expected invalid-yaml summary in stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
}

#[test]
fn fix_with_no_warnings_hides_warning_only_summary() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nnew-line-at-end-of-file = 'disable'\n[rules.comments]\nlevel = 'warning'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--fix")
        .arg("--no-warnings")
        .arg(&file));

    assert_eq!(
        code, 0,
        "warning-only fix should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert_eq!(fs::read_to_string(&file).unwrap(), "key: value  # comment");
}

#[test]
fn fix_missing_file_reports_read_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("missing.yaml");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    assert_eq!(
        code, 2,
        "missing file should fail: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("failed to read"),
        "expected read error in stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
}

#[test]
fn fix_applies_new_safe_spacing_rules() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        &file,
        "root:\n  mapping: {  key: [1 ,2]   }\n  empty: []\n # wrong\n  next: value\n",
    )
    .unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nnew-line-at-end-of-file = 'disable'\ncomments-indentation = 'enable'\ncommas = 'enable'\nbraces = 'enable'\n[rules.brackets]\nmin-spaces-inside-empty = 1\nmax-spaces-inside-empty = 1\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    assert_eq!(
        code, 0,
        "new spacing fixes should succeed: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 6 problems (6 fixed, 0 remaining)."),
        "expected all-new-fixes summary in stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "root:\n  mapping: {key: [1, 2]}\n  empty: [ ]\n  # wrong\n  next: value\n"
    );
}
