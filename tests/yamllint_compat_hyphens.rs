use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn hyphens_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("hyphens-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  hyphens: enable\n",
    )
    .unwrap();

    let warning_cfg = dir.path().join("hyphens-warning.yml");
    fs::write(
        &warning_cfg,
        "rules:\n  document-start: disable\n  hyphens:\n    level: warning\n",
    )
    .unwrap();

    let max_cfg = dir.path().join("hyphens-max.yml");
    fs::write(
        &max_cfg,
        "rules:\n  document-start: disable\n  hyphens:\n    max-spaces-after: 3\n",
    )
    .unwrap();

    let ignore_cfg = dir.path().join("hyphens-ignore.yml");
    fs::write(
        &ignore_cfg,
        "rules:\n  document-start: disable\n  hyphens:\n    ignore:\n      - ignored.yaml\n",
    )
    .unwrap();

    let ignore_list = dir.path().join("hyphens-ignore.txt");
    fs::write(&ignore_list, "ignored-from-file.yaml\n").unwrap();

    let ignore_from_file_cfg = dir.path().join("hyphens-ignore-from-file.yml");
    let ignore_path = ignore_list.display().to_string().replace('\'', "''");
    fs::write(
        &ignore_from_file_cfg,
        format!(
            "rules:\n  document-start: disable\n  hyphens:\n    ignore-from-file: '{}'\n",
            ignore_path
        ),
    )
    .unwrap();

    let default_file = dir.path().join("bad.yaml");
    fs::write(&default_file, "---\n-  item\n").unwrap();

    let max_violation_file = dir.path().join("bad-max.yaml");
    fs::write(&max_violation_file, "---\n-    item\n").unwrap();

    let max_ok_file = dir.path().join("ok-max.yaml");
    fs::write(&max_ok_file, "---\n-   item\n").unwrap();

    let ignored_file = dir.path().join("ignored.yaml");
    fs::write(&ignored_file, "---\n-  item\n").unwrap();

    let ignored_from_file = dir.path().join("ignored-from-file.yaml");
    fs::write(&ignored_from_file, "---\n-  item\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_default.arg("-c").arg(&default_cfg).arg(&default_file);
        let (ryl_default_code, ryl_default_output) = capture_with_env(ryl_default, scenario.envs);

        let mut yam_default = build_yamllint_command(scenario.yam_format);
        yam_default.arg("-c").arg(&default_cfg).arg(&default_file);
        let (yam_default_code, yam_default_output) = capture_with_env(yam_default, scenario.envs);

        assert_eq!(ryl_default_code, 1, "ryl default exit ({})", scenario.label);
        assert_eq!(
            yam_default_code, 1,
            "yamllint default exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_default_output, yam_default_output,
            "default diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_warning = build_ryl_command(exe, scenario.ryl_format);
        ryl_warning.arg("-c").arg(&warning_cfg).arg(&default_file);
        let (ryl_warning_code, ryl_warning_output) = capture_with_env(ryl_warning, scenario.envs);

        let mut yam_warning = build_yamllint_command(scenario.yam_format);
        yam_warning.arg("-c").arg(&warning_cfg).arg(&default_file);
        let (yam_warning_code, yam_warning_output) = capture_with_env(yam_warning, scenario.envs);

        assert_eq!(ryl_warning_code, 0, "ryl warning exit ({})", scenario.label);
        assert_eq!(
            yam_warning_code, 0,
            "yamllint warning exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_warning_output, yam_warning_output,
            "warning diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_max_violation = build_ryl_command(exe, scenario.ryl_format);
        ryl_max_violation
            .arg("-c")
            .arg(&max_cfg)
            .arg(&max_violation_file);
        let (ryl_max_code, ryl_max_output) = capture_with_env(ryl_max_violation, scenario.envs);

        let mut yam_max_violation = build_yamllint_command(scenario.yam_format);
        yam_max_violation
            .arg("-c")
            .arg(&max_cfg)
            .arg(&max_violation_file);
        let (yam_max_code, yam_max_output) = capture_with_env(yam_max_violation, scenario.envs);

        assert_eq!(ryl_max_code, 1, "ryl max exit ({})", scenario.label);
        assert_eq!(yam_max_code, 1, "yamllint max exit ({})", scenario.label);
        assert_eq!(
            ryl_max_output, yam_max_output,
            "max diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_max_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_max_ok.arg("-c").arg(&max_cfg).arg(&max_ok_file);
        let (ryl_max_ok_code, ryl_max_ok_output) = capture_with_env(ryl_max_ok, scenario.envs);

        let mut yam_max_ok = build_yamllint_command(scenario.yam_format);
        yam_max_ok.arg("-c").arg(&max_cfg).arg(&max_ok_file);
        let (yam_max_ok_code, yam_max_ok_output) = capture_with_env(yam_max_ok, scenario.envs);

        assert_eq!(ryl_max_ok_code, 0, "ryl max-ok exit ({})", scenario.label);
        assert_eq!(
            yam_max_ok_code, 0,
            "yamllint max-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_max_ok_output, yam_max_ok_output,
            "max-ok diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_ignore = build_ryl_command(exe, scenario.ryl_format);
        ryl_ignore.arg("-c").arg(&ignore_cfg).arg(&ignored_file);
        let (ryl_ignore_code, ryl_ignore_output) = capture_with_env(ryl_ignore, scenario.envs);

        let mut yam_ignore = build_yamllint_command(scenario.yam_format);
        yam_ignore.arg("-c").arg(&ignore_cfg).arg(&ignored_file);
        let (yam_ignore_code, yam_ignore_output) = capture_with_env(yam_ignore, scenario.envs);

        assert_eq!(ryl_ignore_code, 0, "ryl ignore exit ({})", scenario.label);
        assert_eq!(
            yam_ignore_code, 0,
            "yamllint ignore exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_ignore_output, yam_ignore_output,
            "ignore diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_ignore_file = build_ryl_command(exe, scenario.ryl_format);
        ryl_ignore_file
            .arg("-c")
            .arg(&ignore_from_file_cfg)
            .arg(&ignored_from_file);
        let (ryl_ignore_file_code, ryl_ignore_file_output) =
            capture_with_env(ryl_ignore_file, scenario.envs);

        let mut yam_ignore_file = build_yamllint_command(scenario.yam_format);
        yam_ignore_file
            .arg("-c")
            .arg(&ignore_from_file_cfg)
            .arg(&ignored_from_file);
        let (yam_ignore_file_code, yam_ignore_file_output) =
            capture_with_env(yam_ignore_file, scenario.envs);

        assert_eq!(
            ryl_ignore_file_code, 0,
            "ryl ignore-from-file exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_ignore_file_code, 0,
            "yamllint ignore-from-file exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_ignore_file_output, yam_ignore_file_output,
            "ignore-from-file diagnostics mismatch ({})",
            scenario.label
        );
    }
}
