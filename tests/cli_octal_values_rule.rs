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
fn octal_rule_reports_plain_values() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("values.yaml");
    fs::write(&file, "foo: 010\nbar: 0o10\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  octal-values: enable\n",
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
        output.contains("forbidden implicit octal value \"010\""),
        "missing implicit message: {output}"
    );
    assert!(
        output.contains("forbidden explicit octal value \"0o10\""),
        "missing explicit message: {output}"
    );
    assert!(
        output.contains("octal-values"),
        "rule label missing: {output}"
    );
    assert!(
        output.contains("1:9"),
        "expected implicit octal position: {output}"
    );
    assert!(
        output.contains("2:10"),
        "expected explicit octal position: {output}"
    );
}
