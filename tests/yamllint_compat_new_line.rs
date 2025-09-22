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

fn ensure_yamllint_installed() {
    let ok = Command::new("yamllint")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    assert!(ok, "yamllint must be installed for compatibility tests");
}

fn normalize_output(stdout: String, stderr: String) -> String {
    if stderr.is_empty() { stdout } else { stderr }
}

fn capture_with_env(mut cmd: Command, envs: &[(&str, Option<&str>)]) -> (i32, String) {
    cmd.env_remove("GITHUB_ACTIONS");
    cmd.env_remove("GITHUB_WORKFLOW");
    cmd.env_remove("CI");
    for (key, value) in envs {
        if let Some(v) = value {
            cmd.env(key, v);
        } else {
            cmd.env_remove(key);
        }
    }
    let (code, stdout, stderr) = run(&mut cmd);
    (code, normalize_output(stdout, stderr))
}

#[test]
fn new_line_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yml");
    fs::write(
        &cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: enable\n",
    )
    .unwrap();
    let cfg_warning = dir.path().join("config-warning.yml");
    fs::write(
        &cfg_warning,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file:\n    level: warning\n",
    )
    .unwrap();

    let missing = dir.path().join("missing.yaml");
    fs::write(&missing, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    const STANDARD_ENV: &[(&str, Option<&str>)] = &[];
    const GITHUB_ENV: &[(&str, Option<&str>)] = &[
        ("GITHUB_ACTIONS", Some("true")),
        ("GITHUB_WORKFLOW", Some("test-workflow")),
        ("CI", Some("true")),
    ];
    let scenarios = [("standard", STANDARD_ENV), ("github", GITHUB_ENV)];

    let invalid = dir.path().join("invalid.yaml");
    fs::write(&invalid, "key: [1").unwrap();

    for (label, envs) in scenarios {
        let mut ryl_missing_cmd = Command::new(exe);
        ryl_missing_cmd.arg("-c").arg(&cfg).arg(&missing);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_missing_cmd, envs);

        let mut yam_missing_cmd = Command::new("yamllint");
        yam_missing_cmd.arg("-c").arg(&cfg).arg(&missing);
        let (yam_code, yam_msg) = capture_with_env(yam_missing_cmd, envs);

        assert_eq!(ryl_code, 1, "ryl exit code for missing newline ({label})");
        assert_eq!(
            yam_code, 1,
            "yamllint exit code for missing newline ({label})",
        );
        assert_eq!(ryl_msg, yam_msg, "expected identical diagnostics ({label})");

        let mut ryl_invalid_cmd = Command::new(exe);
        ryl_invalid_cmd.arg("-c").arg(&cfg).arg(&invalid);
        let (ryl_bad_code, ryl_bad) = capture_with_env(ryl_invalid_cmd, envs);

        let mut yam_invalid_cmd = Command::new("yamllint");
        yam_invalid_cmd.arg("-c").arg(&cfg).arg(&invalid);
        let (yam_bad_code, yam_bad) = capture_with_env(yam_invalid_cmd, envs);

        assert_eq!(ryl_bad_code, 1, "ryl exit code for invalid yaml ({label})");
        assert_eq!(
            yam_bad_code, 1,
            "yamllint exit code for invalid yaml ({label})",
        );
        assert!(
            ryl_bad.contains("syntax error"),
            "ryl should report a syntax error ({label}): {ryl_bad}"
        );
        assert!(
            yam_bad.contains("syntax error"),
            "yamllint should report a syntax error ({label}): {yam_bad}"
        );
        assert!(
            !ryl_bad.contains("no new line character"),
            "new line rule should be suppressed when syntax fails ({label}): {ryl_bad}"
        );
        assert!(
            !yam_bad.contains("no new line character"),
            "yamllint should suppress new line rule when syntax fails ({label}): {yam_bad}"
        );

        let mut ryl_warning_cmd = Command::new(exe);
        ryl_warning_cmd.arg("-c").arg(&cfg_warning).arg(&missing);
        let (ryl_warn_code, ryl_warn) = capture_with_env(ryl_warning_cmd, envs);

        let mut yam_warning_cmd = Command::new("yamllint");
        yam_warning_cmd.arg("-c").arg(&cfg_warning).arg(&missing);
        let (yam_warn_code, yam_warn) = capture_with_env(yam_warning_cmd, envs);

        assert_eq!(
            ryl_warn_code, 0,
            "ryl exit code for warning-level rule ({label})"
        );
        assert_eq!(
            yam_warn_code, 0,
            "yamllint exit code for warning-level rule ({label})",
        );
        assert_eq!(
            ryl_warn, yam_warn,
            "expected identical warning diagnostics ({label})",
        );
        assert!(
            ryl_warn.contains("warning"),
            "warning output should mention warning ({label}): {ryl_warn}"
        );
    }
}
