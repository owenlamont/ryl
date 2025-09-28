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
fn empty_values_reports_all_diagnostics() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("invalid.yaml");
    fs::write(&file, "block:\n  missing:\nflow: { missing: }\nseq:\n  -\n").unwrap();

    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  empty-values: enable\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { &stdout } else { &stderr };
    assert!(
        output.contains("empty value in block mapping"),
        "missing block mapping message: {output}"
    );
    assert!(
        output.contains("empty value in flow mapping"),
        "missing flow mapping message: {output}"
    );
    assert!(
        output.contains("empty value in block sequence"),
        "missing block sequence message: {output}"
    );
}

#[test]
fn empty_values_honors_flags() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ok.yaml");
    fs::write(&file, "block:\n  missing:\nflow: { missing: }\nseq:\n  -\n").unwrap();

    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  empty-values:\n    forbid-in-block-mappings: false\n    forbid-in-flow-mappings: false\n    forbid-in-block-sequences: false\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "expected success when all checks disabled: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}
