use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn anchors_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("anchors-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  anchors: enable\n",
    )
    .unwrap();

    let allow_forward_cfg = dir.path().join("anchors-allow-forward.yml");
    fs::write(
        &allow_forward_cfg,
        "rules:\n  document-start: disable\n  anchors:\n    forbid-undeclared-aliases: false\n",
    )
    .unwrap();

    let duplicates_cfg = dir.path().join("anchors-duplicates.yml");
    fs::write(
        &duplicates_cfg,
        "rules:\n  document-start: disable\n  anchors:\n    forbid-undeclared-aliases: false\n    forbid-duplicated-anchors: true\n",
    )
    .unwrap();

    let unused_cfg = dir.path().join("anchors-unused.yml");
    fs::write(
        &unused_cfg,
        "rules:\n  document-start: disable\n  anchors:\n    forbid-unused-anchors: true\n",
    )
    .unwrap();

    let valid_file = dir.path().join("valid.yaml");
    fs::write(&valid_file, "---\n- &anchor value\n- *anchor\n").unwrap();

    let undeclared_file = dir.path().join("undeclared.yaml");
    fs::write(&undeclared_file, "---\n- *missing\n- &missing value\n").unwrap();

    let duplicate_file = dir.path().join("duplicate.yaml");
    fs::write(&duplicate_file, "---\n- &anchor one\n- &anchor two\n").unwrap();

    let unused_file = dir.path().join("unused.yaml");
    fs::write(&unused_file, "---\n- &anchor value\n- 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        // Default config: undeclared alias should fail
        let mut ryl_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_default
            .arg("-c")
            .arg(&default_cfg)
            .arg(&undeclared_file);
        let (ryl_default_code, ryl_default_output) = capture_with_env(ryl_default, scenario.envs);

        let mut yam_default = build_yamllint_command(scenario.yam_format);
        yam_default
            .arg("-c")
            .arg(&default_cfg)
            .arg(&undeclared_file);
        let (yam_default_code, yam_default_output) = capture_with_env(yam_default, scenario.envs);

        assert_eq!(
            ryl_default_code, 1,
            "ryl undeclared exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_default_code, 1,
            "yamllint undeclared exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_default_output, yam_default_output,
            "undeclared diagnostics mismatch ({})",
            scenario.label
        );

        // Forward alias allowed when disabled
        let mut ryl_allow = build_ryl_command(exe, scenario.ryl_format);
        ryl_allow
            .arg("-c")
            .arg(&allow_forward_cfg)
            .arg(&undeclared_file);
        let (ryl_allow_code, ryl_allow_output) = capture_with_env(ryl_allow, scenario.envs);

        let mut yam_allow = build_yamllint_command(scenario.yam_format);
        yam_allow
            .arg("-c")
            .arg(&allow_forward_cfg)
            .arg(&undeclared_file);
        let (yam_allow_code, yam_allow_output) = capture_with_env(yam_allow, scenario.envs);

        assert_eq!(
            ryl_allow_code, 0,
            "ryl allow-forward exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_allow_code, 0,
            "yamllint allow-forward exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_allow_output, yam_allow_output,
            "allow-forward diagnostics mismatch ({})",
            scenario.label
        );

        // Duplicate anchors
        let mut ryl_duplicate = build_ryl_command(exe, scenario.ryl_format);
        ryl_duplicate
            .arg("-c")
            .arg(&duplicates_cfg)
            .arg(&duplicate_file);
        let (ryl_duplicate_code, ryl_duplicate_output) =
            capture_with_env(ryl_duplicate, scenario.envs);

        let mut yam_duplicate = build_yamllint_command(scenario.yam_format);
        yam_duplicate
            .arg("-c")
            .arg(&duplicates_cfg)
            .arg(&duplicate_file);
        let (yam_duplicate_code, yam_duplicate_output) =
            capture_with_env(yam_duplicate, scenario.envs);

        assert_eq!(
            ryl_duplicate_code, 1,
            "ryl duplicate exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_duplicate_code, 1,
            "yamllint duplicate exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_duplicate_output, yam_duplicate_output,
            "duplicate diagnostics mismatch ({})",
            scenario.label
        );

        // Unused anchors
        let mut ryl_unused = build_ryl_command(exe, scenario.ryl_format);
        ryl_unused.arg("-c").arg(&unused_cfg).arg(&unused_file);
        let (ryl_unused_code, ryl_unused_output) = capture_with_env(ryl_unused, scenario.envs);

        let mut yam_unused = build_yamllint_command(scenario.yam_format);
        yam_unused.arg("-c").arg(&unused_cfg).arg(&unused_file);
        let (yam_unused_code, yam_unused_output) = capture_with_env(yam_unused, scenario.envs);

        assert_eq!(ryl_unused_code, 1, "ryl unused exit ({})", scenario.label);
        assert_eq!(
            yam_unused_code, 1,
            "yamllint unused exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_unused_output, yam_unused_output,
            "unused diagnostics mismatch ({})",
            scenario.label
        );

        // Valid file should pass
        let mut ryl_valid = build_ryl_command(exe, scenario.ryl_format);
        ryl_valid.arg("-c").arg(&default_cfg).arg(&valid_file);
        let (ryl_valid_code, ryl_valid_output) = capture_with_env(ryl_valid, scenario.envs);

        let mut yam_valid = build_yamllint_command(scenario.yam_format);
        yam_valid.arg("-c").arg(&default_cfg).arg(&valid_file);
        let (yam_valid_code, yam_valid_output) = capture_with_env(yam_valid, scenario.envs);

        assert_eq!(ryl_valid_code, 0, "ryl valid exit ({})", scenario.label);
        assert_eq!(
            yam_valid_code, 0,
            "yamllint valid exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_valid_output, yam_valid_output,
            "valid diagnostics mismatch ({})",
            scenario.label
        );
    }
}
