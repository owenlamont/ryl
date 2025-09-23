use std::fs;
use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

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

    let invalid = dir.path().join("invalid.yaml");
    fs::write(&invalid, "key: [1").unwrap();

    for scenario in SCENARIOS {
        let mut ryl_missing_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_missing_cmd.arg("-c").arg(&cfg).arg(&missing);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_missing_cmd, scenario.envs);

        let mut yam_missing_cmd = build_yamllint_command(scenario.yam_format);
        yam_missing_cmd.arg("-c").arg(&cfg).arg(&missing);
        let (yam_code, yam_msg) = capture_with_env(yam_missing_cmd, scenario.envs);

        assert_eq!(
            ryl_code, 1,
            "ryl exit code for missing newline ({})",
            scenario.label
        );
        assert_eq!(
            yam_code, 1,
            "yamllint exit code for missing newline ({})",
            scenario.label
        );
        assert_eq!(
            ryl_msg, yam_msg,
            "expected identical diagnostics ({})",
            scenario.label
        );

        let mut ryl_invalid_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_invalid_cmd.arg("-c").arg(&cfg).arg(&invalid);
        let (ryl_bad_code, ryl_bad) = capture_with_env(ryl_invalid_cmd, scenario.envs);

        let mut yam_invalid_cmd = build_yamllint_command(scenario.yam_format);
        yam_invalid_cmd.arg("-c").arg(&cfg).arg(&invalid);
        let (yam_bad_code, yam_bad) = capture_with_env(yam_invalid_cmd, scenario.envs);

        assert_eq!(
            ryl_bad_code, 1,
            "ryl exit code for invalid yaml ({})",
            scenario.label
        );
        assert_eq!(
            yam_bad_code, 1,
            "yamllint exit code for invalid yaml ({})",
            scenario.label
        );
        assert!(
            ryl_bad.contains("syntax error"),
            "ryl should report a syntax error ({}): {ryl_bad}",
            scenario.label
        );
        assert!(
            yam_bad.contains("syntax error"),
            "yamllint should report a syntax error ({}): {yam_bad}",
            scenario.label
        );
        assert!(
            !ryl_bad.contains("no new line character"),
            "new line rule should be suppressed when syntax fails ({}): {ryl_bad}",
            scenario.label
        );
        assert!(
            !yam_bad.contains("no new line character"),
            "yamllint should suppress new line rule when syntax fails ({}): {yam_bad}",
            scenario.label
        );

        let mut ryl_warning_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_warning_cmd.arg("-c").arg(&cfg_warning).arg(&missing);
        let (ryl_warn_code, ryl_warn) = capture_with_env(ryl_warning_cmd, scenario.envs);

        let mut yam_warning_cmd = build_yamllint_command(scenario.yam_format);
        yam_warning_cmd.arg("-c").arg(&cfg_warning).arg(&missing);
        let (yam_warn_code, yam_warn) = capture_with_env(yam_warning_cmd, scenario.envs);

        assert_eq!(
            ryl_warn_code, 0,
            "ryl exit code for warning-level rule ({})",
            scenario.label
        );
        assert_eq!(
            yam_warn_code, 0,
            "yamllint exit code for warning-level rule ({})",
            scenario.label
        );
        assert_eq!(
            ryl_warn, yam_warn,
            "expected identical warning diagnostics ({})",
            scenario.label
        );
        assert!(
            ryl_warn.contains("warning"),
            "warning output should mention warning ({}): {ryl_warn}",
            scenario.label
        );
    }
}
