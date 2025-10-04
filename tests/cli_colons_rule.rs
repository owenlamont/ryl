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
fn colons_reports_spacing_errors() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "---\nkey :  value\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  colons: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("too many spaces before colon"),
        "missing before-colon message: {output}"
    );
    assert!(
        output.contains("too many spaces after colon"),
        "missing after-colon message: {output}"
    );
    assert!(output.contains("colons"), "rule id missing: {output}");
}

#[test]
fn colons_reports_explicit_key_spacing() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("explicit.yaml");
    fs::write(&file, "---\n?  key\n: value\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  colons: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("too many spaces after question mark"),
        "missing question mark message: {output}"
    );
}
