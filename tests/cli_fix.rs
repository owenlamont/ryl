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

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: value  # comment");
}
