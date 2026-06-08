use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

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
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
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
