use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn comments_indentation_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let enable_cfg = dir.path().join("comments-enable.yml");
    fs::write(
        &enable_cfg,
        "rules:\n  document-start: disable\n  comments-indentation: enable\n",
    )
    .unwrap();

    let warning_cfg = dir.path().join("comments-warning.yml");
    fs::write(
        &warning_cfg,
        "rules:\n  document-start: disable\n  comments-indentation:\n    level: warning\n",
    )
    .unwrap();

    let ignore_cfg = dir.path().join("comments-ignore.yml");
    fs::write(
        &ignore_cfg,
        "rules:\n  document-start: disable\n  comments-indentation:\n    ignore:\n      - ignored.yaml\n",
    )
    .unwrap();

    let ignore_list = dir.path().join("comments-ignore.txt");
    fs::write(&ignore_list, "ignored-from-file.yaml\n").unwrap();

    let ignore_from_file_cfg = dir.path().join("comments-ignore-from-file.yml");
    let ignore_path = ignore_list.display().to_string().replace('\'', "''");
    fs::write(
        &ignore_from_file_cfg,
        format!(
            "rules:\n  document-start: disable\n  comments-indentation:\n    ignore-from-file: '{}'\n",
            ignore_path
        ),
    )
    .unwrap();

    let bad_file = dir.path().join("bad.yaml");
    fs::write(&bad_file, "obj:\n # wrong\n  value: 1\n").unwrap();

    let ignored_file = dir.path().join("ignored.yaml");
    fs::write(&ignored_file, "obj:\n # wrong\n  value: 1\n").unwrap();

    let ignored_from_file = dir.path().join("ignored-from-file.yaml");
    fs::write(&ignored_from_file, "obj:\n # wrong\n  value: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_enable = build_ryl_command(exe, scenario.ryl_format);
        ryl_enable.arg("-c").arg(&enable_cfg).arg(&bad_file);
        let (ryl_code, ryl_output) = capture_with_env(ryl_enable, scenario.envs);

        let mut yam_enable = build_yamllint_command(scenario.yam_format);
        yam_enable.arg("-c").arg(&enable_cfg).arg(&bad_file);
        let (yam_code, yam_output) = capture_with_env(yam_enable, scenario.envs);

        assert_eq!(ryl_code, 1, "ryl enable exit ({})", scenario.label);
        assert_eq!(yam_code, 1, "yamllint enable exit ({})", scenario.label);
        assert_eq!(
            ryl_output, yam_output,
            "enable diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_warning = build_ryl_command(exe, scenario.ryl_format);
        ryl_warning.arg("-c").arg(&warning_cfg).arg(&bad_file);
        let (ryl_warn_code, ryl_warn_output) = capture_with_env(ryl_warning, scenario.envs);

        let mut yam_warning = build_yamllint_command(scenario.yam_format);
        yam_warning.arg("-c").arg(&warning_cfg).arg(&bad_file);
        let (yam_warn_code, yam_warn_output) = capture_with_env(yam_warning, scenario.envs);

        assert_eq!(ryl_warn_code, 0, "ryl warning exit ({})", scenario.label);
        assert_eq!(
            yam_warn_code, 0,
            "yamllint warning exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_warn_output, yam_warn_output,
            "warning diagnostics mismatch ({})",
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
        let (ryl_file_code, ryl_file_output) = capture_with_env(ryl_ignore_file, scenario.envs);

        let mut yam_ignore_file = build_yamllint_command(scenario.yam_format);
        yam_ignore_file
            .arg("-c")
            .arg(&ignore_from_file_cfg)
            .arg(&ignored_from_file);
        let (yam_file_code, yam_file_output) = capture_with_env(yam_ignore_file, scenario.envs);

        assert_eq!(
            ryl_file_code, 0,
            "ryl ignore-from-file exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_file_code, 0,
            "yamllint ignore-from-file exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_file_output, yam_file_output,
            "ignore-from-file diagnostics mismatch ({})",
            scenario.label
        );
    }
}
