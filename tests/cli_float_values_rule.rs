use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

#[test]
fn float_values_rule_reports_forbidden_variants() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("values.yaml");
    fs::write(&file, "a: .5\nb: 1e2\nc: .nan\nd: .inf\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  float-values:\n    require-numeral-before-decimal: true\n    forbid-scientific-notation: true\n    forbid-nan: true\n    forbid-inf: true\n",
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
        output.contains("forbidden decimal missing 0 prefix \".5\""),
        "missing decimal prefix message: {output}"
    );
    assert!(
        output.contains("forbidden scientific notation \"1e2\""),
        "missing scientific notation message: {output}"
    );
    assert!(
        output.contains("forbidden not a number value \".nan\""),
        "missing nan message: {output}"
    );
    assert!(
        output.contains("forbidden infinite value \".inf\""),
        "missing inf message: {output}"
    );
    assert!(
        output.contains("float-values"),
        "rule label missing: {output}"
    );
    assert!(output.contains("1:4"), "expected .5 position: {output}");
    assert!(output.contains("2:4"), "expected 1e2 position: {output}");
    assert!(output.contains("3:4"), "expected .nan position: {output}");
    assert!(output.contains("4:4"), "expected .inf position: {output}");
}
