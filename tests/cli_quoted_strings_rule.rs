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
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
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

#[test]
fn toml_allows_escaped_double_quotes_as_single_quote_exception() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.yaml");
    fs::write(&file, "escaped: \"line\\nbreak\"\nplain: \"text\"\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\nallow-double-quotes-for-escaping = true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 1,
        "only unescaped double quotes should fail: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("2:8"),
        "plain double-quoted scalar should be reported: {output}"
    );
    assert!(
        !output.contains("1:10"),
        "escaped double-quoted scalar should be allowed: {output}"
    );
}

#[test]
fn escaped_double_quote_exception_does_not_set_consistent_style() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.yaml");
    fs::write(
        &file,
        "escaped: \"line\\nbreak\"\nfirst: 'text'\nsecond: \"text\"\n",
    )
    .unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.quoted-strings]\nquote-type = 'consistent'\nrequired = false\nallow-double-quotes-for-escaping = true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 1,
        "unescaped double quotes should fail after single quote baseline: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("3:9"),
        "unescaped double-quoted scalar should be reported: {output}"
    );
    assert!(
        !output.contains("1:10") && !output.contains("2:8"),
        "escaped exception and single quote baseline should pass: {output}"
    );
}
