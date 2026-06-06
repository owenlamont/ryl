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
fn key_ordering_reports_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "block mapping:\n  second: value\n  first: value\n").unwrap();

    let cfg = dir.path().join("config.yml");
    fs::write(
        &cfg,
        "rules:\n  document-start: disable\n  key-ordering: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&cfg).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("wrong ordering of key \"first\" in mapping"),
        "missing message: {output}"
    );
    assert!(output.contains("key-ordering"), "rule id missing: {output}");
}

#[test]
fn ignored_keys_skip_enforcement() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "name: zed\nfirst-name: zed\na: 1\n").unwrap();

    let cfg = dir.path().join("config.yml");
    fs::write(
        &cfg,
        "rules:\n  document-start: disable\n  key-ordering:\n    ignored-keys: [\"name\", \"first-name\"]\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&cfg).arg(&file));
    assert_eq!(
        code, 0,
        "ignored keys should not fail: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn alias_value_does_not_desync_key_ordering() {
    // An alias in value position is a full node; if the walker is not advanced
    // past it the key/value alternation desyncs and the next value is misread as
    // an out-of-order key reported past end-of-file (regression: a `<<: *m0`
    // merge value followed by another key surfaced via the property suite).
    let dir = tempdir().unwrap();
    let file = dir.path().join("alias.yaml");
    fs::write(&file, "base: &m0 {dup: 1}\nhost:\n  <<: *m0\n  z: ok\n").unwrap();

    let cfg = dir.path().join("config.yml");
    fs::write(
        &cfg,
        "rules:\n  document-start: disable\n  key-ordering: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&cfg).arg(&file));
    assert_eq!(
        code, 0,
        "an alias value must not desync ordering: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
}
