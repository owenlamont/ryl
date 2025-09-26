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

fn command_output<'a>(stdout: &'a str, stderr: &'a str) -> &'a str {
    if stderr.is_empty() { stdout } else { stderr }
}

#[test]
fn duplicate_keys_reported() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("dup.yaml");
    fs::write(&file, "foo: 1\nbar: 2\nfoo: 3\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  key-duplicates: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("duplication of key \"foo\" in mapping"),
        "missing key duplication message: {output}"
    );
    assert!(
        output.contains("key-duplicates"),
        "rule id missing: {output}"
    );
}

#[test]
fn merge_keys_allowed_by_default() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("merge.yaml");
    fs::write(
        &file,
        "anchor: &a\n  value: 1\nmerged:\n  <<: *a\n  <<: *a\n",
    )
    .unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  key-duplicates: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "merge keys allowed by default: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn merge_keys_forbidden_when_configured() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("merge.yaml");
    fs::write(
        &file,
        "anchor: &a\n  value: 1\nmerged:\n  <<: *a\n  <<: *a\n",
    )
    .unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  key-duplicates:\n    forbid-duplicated-merge-keys: true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("duplication of key \"<<\" in mapping"),
        "missing merge duplication message: {output}"
    );
    assert!(
        output.contains("key-duplicates"),
        "rule id missing: {output}"
    );
}
