use std::fs;
use std::process::Command;

use tempfile::tempdir;

struct CliSuite {
    rule_name: &'static str,
    spaced_input: &'static str,
    compact_input: &'static str,
    empty_input: &'static str,
    spacing_message: &'static str,
    forbid_message: &'static str,
    empty_message: &'static str,
}

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn assert_command_output(
    cmd: &mut Command,
    expected_code: i32,
    expected_substrings: &[&str],
) {
    let (code, stdout, stderr) = run(cmd);
    assert_eq!(code, expected_code, "stdout={stdout} stderr={stderr}");
    let output = if stderr.is_empty() { stdout } else { stderr };
    for needle in expected_substrings {
        assert!(output.contains(needle), "missing message: {output}");
    }
}

fn run_cli_suite(suite: CliSuite) {
    let dir = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");

    let bad = dir.path().join("bad.yaml");
    fs::write(&bad, suite.spaced_input).unwrap();
    assert_command_output(
        Command::new(exe).arg(&bad),
        1,
        &[suite.spacing_message, suite.rule_name],
    );

    let flow = dir.path().join("flow.yaml");
    fs::write(&flow, suite.compact_input).unwrap();
    let forbid = dir.path().join("forbid.yaml");
    fs::write(
        &forbid,
        format!(
            "rules:\n  document-start: disable\n  {}:\n    forbid: true\n",
            suite.rule_name
        ),
    )
    .unwrap();
    assert_command_output(
        Command::new(exe).arg("-c").arg(&forbid).arg(&flow),
        1,
        &[suite.forbid_message],
    );

    let empty = dir.path().join("empty.yaml");
    fs::write(&empty, suite.empty_input).unwrap();
    let empty_cfg = dir.path().join("empty-config.yaml");
    fs::write(
        &empty_cfg,
        format!(
            "rules:\n  document-start: disable\n  {}:\n    min-spaces-inside-empty: 1\n",
            suite.rule_name
        ),
    )
    .unwrap();
    assert_command_output(
        Command::new(exe).arg("-c").arg(&empty_cfg).arg(&empty),
        1,
        &[suite.empty_message],
    );

    let warn = dir.path().join("warn.yaml");
    fs::write(&warn, suite.spaced_input).unwrap();
    let warn_cfg = dir.path().join("warn-config.yaml");
    fs::write(
        &warn_cfg,
        format!(
            "rules:\n  document-start: disable\n  {}:\n    level: warning\n",
            suite.rule_name
        ),
    )
    .unwrap();
    assert_command_output(
        Command::new(exe).arg("-c").arg(&warn_cfg).arg(&warn),
        0,
        &["warning"],
    );
}

#[test]
fn braces_cli_suite() {
    run_cli_suite(CliSuite {
        rule_name: "braces",
        spaced_input: "---\nobject: { key: 1 }\n",
        compact_input: "---\nobject: {key: 1}\n",
        empty_input: "---\nobject: {}\n",
        spacing_message: "too many spaces inside braces",
        forbid_message: "forbidden flow mapping",
        empty_message: "too few spaces inside empty braces",
    });
}

#[test]
fn brackets_cli_suite() {
    run_cli_suite(CliSuite {
        rule_name: "brackets",
        spaced_input: "---\nobject: [ 1, 2 ]\n",
        compact_input: "---\nobject: [1, 2]\n",
        empty_input: "---\nobject: []\n",
        spacing_message: "too many spaces inside brackets",
        forbid_message: "forbidden flow sequence",
        empty_message: "too few spaces inside empty brackets",
    });
}
