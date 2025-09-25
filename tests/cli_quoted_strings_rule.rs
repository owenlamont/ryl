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
fn quoted_strings_reports_redundant_quotes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.yaml");
    fs::write(&file, "foo: \"bar\"\n").unwrap();

    let config = dir.path().join("config.yaml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
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
        output.contains("string value is redundantly quoted with any quotes"),
        "missing redundant quote message: {output}"
    );
    assert!(
        output.contains("quoted-strings"),
        "rule label missing: {output}"
    );
}
