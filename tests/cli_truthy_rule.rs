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
fn truthy_rule_reports_plain_truthy_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("values.yaml");
    fs::write(&file, "foo: True\nbar: yes\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  truthy: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 1,
        "expected lint failure: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("truthy value should be one of [false, true]"),
        "missing truthy message: {output}"
    );
    assert!(output.contains("truthy"), "rule label missing: {output}");
    assert!(
        output.contains("1:6"),
        "expected position for value 'True': {output}"
    );
    assert!(
        output.contains("2:6"),
        "expected position for value 'yes': {output}"
    );
}

#[test]
fn truthy_rule_respects_check_keys_false() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("keys.yaml");
    fs::write(&file, "True: yes\nvalue: True\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  truthy:\n    allowed-values: []\n    check-keys: false\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 1,
        "expected lint failure: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("2:8"),
        "value position should be reported when keys are skipped: {output}"
    );
    assert!(
        !output.contains("1:1"),
        "keys should be ignored when disabled: {output}"
    );
}
