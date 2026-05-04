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
fn fix_preserves_escaped_double_quotes_in_toml_config() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.yaml");
    fs::write(&file, "key: \"line\\nbreak\"\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\nallow-double-quotes-for-escaping = true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, _stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&config)
        .arg("--fix")
        .arg(&file));
    assert_eq!(code, 0);
    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: \"line\\nbreak\"\n");
}

#[test]
fn fix_preserves_escaped_tabs_that_would_be_invalid_plain_scalars() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.yaml");
    fs::write(&file, "key: \"a\\tb\"\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.quoted-strings]\nrequired = 'only-when-needed'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&config)
        .arg("--fix")
        .arg(&file));
    assert_eq!(
        code, 0,
        "escaped tab should stay valid after fix: stdout={stdout} stderr={stderr}"
    );
    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: \"a\\tb\"\n");
}

#[test]
fn fix_removes_redundant_quotes_with_cli_fix_flag() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.yaml");
    fs::write(&file, "key: \"value\"\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, _stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&config)
        .arg("--fix")
        .arg(&file));
    assert_eq!(code, 0);
    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: value\n");
}

#[test]
fn fix_preserves_inline_comments_when_removing_quotes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.yaml");
    fs::write(&file, "cron: \"daily\" # Some schedule\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, _stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&config)
        .arg("--fix")
        .arg(&file));
    assert_eq!(code, 0);
    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "cron: daily # Some schedule\n");
}

#[test]
fn fix_keeps_quotes_for_plain_scalar_edge_cases() {
    let dir = tempdir().unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let cases = [
        ("cron", "cron: '30 21 * * 0'\n"),
        ("wildcard-token", "value: 'foo * bar'\n"),
        ("anchor-token", "value: 'foo & bar'\n"),
        ("tag-token", "value: 'foo ! bar'\n"),
        ("literal-token", "value: 'foo | bar'\n"),
        ("folded-token", "value: 'foo > bar'\n"),
        ("mapping-key-token", "value: 'foo ? bar'\n"),
        ("reserved-at", "value: 'foo @ bar'\n"),
        ("reserved-percent", "value: 'foo % bar'\n"),
        ("reserved-backtick", "value: 'foo ` bar'\n"),
        ("comment", "value: 'foo # bar'\n"),
        ("mapping", "value: 'foo: bar'\n"),
        ("flow-sequence", "value: '[1, 2]'\n"),
        ("flow-mapping", "value: '{a: 1}'\n"),
    ];

    for (label, input) in cases {
        let file = dir.path().join(format!("{label}.yaml"));
        fs::write(&file, input).unwrap();

        let (code, stdout, stderr) =
            run(Command::new(exe).arg("-c").arg(&config).arg(&file));
        assert_eq!(
            code, 0,
            "quoted scalar should remain valid for {label}: stdout={stdout} stderr={stderr}"
        );

        let (code, _stdout, _stderr) = run(Command::new(exe)
            .arg("-c")
            .arg(&config)
            .arg("--fix")
            .arg(&file));
        assert_eq!(code, 0, "fix should succeed for {label}");
        let fixed = fs::read_to_string(&file).unwrap();
        assert_eq!(fixed, input, "fix should preserve quotes for {label}");
    }
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

#[test]
fn fix_consistent_ignores_escaped_exception_when_seeding_style() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("data.yaml");
    fs::write(
        &file,
        "escaped: \"line\\nbreak\"\nplain: value\nquoted: 'two'\n",
    )
    .unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules.quoted-strings]\nquote-type = 'consistent'\nallow-double-quotes-for-escaping = true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, _stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&config)
        .arg("--fix")
        .arg(&file));
    assert_eq!(code, 0);
    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed,
        "escaped: \"line\\nbreak\"\nplain: 'value'\nquoted: 'two'\n"
    );
}
