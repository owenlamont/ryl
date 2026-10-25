use std::fs;
use std::process::Command;

use tempfile::tempdir;

use crate::common;
use common::cli::run;

#[test]
fn commas_reports_errors() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.yaml");
    fs::write(&file, "---\n[1,2]\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("check")
        .arg("-d")
        .arg("rules:\n  commas: enable\n")
        .arg(&file));
    assert_eq!(code, 1, "expected failure: stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("too few spaces after comma"),
        "missing message: {output}"
    );
    assert!(
        output.contains("commas"),
        "rule id missing from output: {output}"
    );
}

#[test]
fn warning_level_does_not_fail() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("warn.yaml");
    fs::write(&file, "---\n[1,2]\n").unwrap();
    let config = dir.path().join("config.yml");
    fs::write(
        &config,
        "rules:\n  document-start: disable\n  commas:\n    level: warning\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("check")
        .arg("-c")
        .arg(&config)
        .arg(&file));
    assert_eq!(
        code, 0,
        "warnings should not fail: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("warning"),
        "expected warning output: {output}"
    );
}

#[test]
fn rule_ignore_skips_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "---\n[1,2]\n").unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        "[lint.rules]\ndocument-start = \"disable\"\ncommas = { ignore = [\"ignored.yaml\"] }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("check")
        .arg("-c")
        .arg(&config)
        .arg(&file));
    assert_eq!(
        code, 0,
        "ignored file should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn relaxed_spacing_allows_compact_flow() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("compact.yaml");
    fs::write(&file, "---\n[1,2]\n").unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        "[lint.rules]\ndocument-start = \"disable\"\ncommas = { max-spaces-before = -1, min-spaces-after = 0, max-spaces-after = -1 }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("check")
        .arg("-c")
        .arg(&config)
        .arg(&file));
    assert_eq!(
        code, 0,
        "relaxed spacing should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn double_curly_template_is_ignored() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("template.yaml");
    fs::write(&file, "---\nvalue: {{ foo(1,2) }}\n").unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        "[lint.rules]\ndocument-start = \"disable\"\ncommas = \"enable\"\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("check")
        .arg("-c")
        .arg(&config)
        .arg(&file));
    assert_eq!(
        code, 0,
        "template commas should be ignored: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn fix_leaves_double_curly_template_unchanged() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("template.yaml");
    fs::write(&file, "value: {{ foo(1,2) }}\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[lint.rules]\ndocument-start = 'disable'\ncommas = 'enable'\nnew-line-at-end-of-file = 'disable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("check").arg("--fix").arg(&file));
    assert_eq!(
        code, 0,
        "template fix should be a no-op: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "value: {{ foo(1,2) }}\n"
    );
}

#[test]
fn fix_drops_a_trailing_comma_the_default_preset_contradicts() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("trailing.yaml");
    let config = "extends: default\n";
    let input = "---\ncafé: [one,]\nmap: {a: 1,}\n";
    fs::write(&file, input).unwrap();
    let ryl = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ryl"));
        command.args(["check", "-d", config]);
        command
    };

    let (code, stdout, stderr) = run(ryl()
        .arg("--diff")
        .arg("-")
        .stdin(fs::File::open(&file).unwrap()));
    assert_eq!(code, 1, "{stderr}");
    assert!(stdout.contains("+café: [one]\n+map: {a: 1}"), "{stdout}");

    let (code, _, stderr) = run(ryl().arg("--fix").arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\ncafé: [one]\nmap: {a: 1}\n"
    );
}

#[test]
fn closing_commas_in_fix_take_the_closing_rule_spacing_unless_suppressed() {
    let dir = tempdir().unwrap();
    for (input, config, expected) in [
        (
            "a: {b: 1,  }\n",
            "rules:\n  commas:\n    min-spaces-after: 0\n  braces: enable\n",
            "a: {b: 1,}\n",
        ),
        (
            "c: [d,  ]  # yamllint disable-line rule:commas\n",
            "rules:\n  commas: enable\n  brackets: enable\n",
            "c: [d,]  # yamllint disable-line rule:commas\n",
        ),
    ] {
        let file = dir.path().join("closing.yaml");
        fs::write(&file, input).unwrap();
        let (code, _, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
            .args(["check", "--fix", "-d", config])
            .arg(&file));
        assert_eq!(code, 0, "{input:?}: {stderr}");
        assert_eq!(fs::read_to_string(&file).unwrap(), expected);
    }
}
