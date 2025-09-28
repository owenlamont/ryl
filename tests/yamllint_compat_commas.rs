use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn commas_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("commas-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  commas: enable\n",
    )
    .unwrap();

    let warning_cfg = dir.path().join("commas-warning.yml");
    fs::write(
        &warning_cfg,
        "rules:\n  document-start: disable\n  commas:\n    level: warning\n",
    )
    .unwrap();

    let before_cfg = dir.path().join("commas-before.yml");
    fs::write(
        &before_cfg,
        "rules:\n  document-start: disable\n  commas:\n    max-spaces-before: 2\n",
    )
    .unwrap();

    let after_cfg = dir.path().join("commas-after.yml");
    fs::write(
        &after_cfg,
        "rules:\n  document-start: disable\n  commas:\n    max-spaces-after: 3\n",
    )
    .unwrap();

    let min_cfg = dir.path().join("commas-min.yml");
    fs::write(
        &min_cfg,
        "rules:\n  document-start: disable\n  commas:\n    min-spaces-after: 2\n    max-spaces-after: -1\n",
    )
    .unwrap();

    let ignore_cfg = dir.path().join("commas-ignore.yml");
    fs::write(
        &ignore_cfg,
        "rules:\n  document-start: disable\n  commas:\n    ignore:\n      - ignored.yaml\n",
    )
    .unwrap();

    let ignore_list = dir.path().join("commas-ignore.txt");
    fs::write(&ignore_list, "ignored-from-file.yaml\n").unwrap();

    let ignore_from_file_cfg = dir.path().join("commas-ignore-from-file.yml");
    let ignore_path = ignore_list.display().to_string().replace('\'', "''");
    fs::write(
        &ignore_from_file_cfg,
        format!(
            "rules:\n  document-start: disable\n  commas:\n    ignore-from-file: '{}'\n",
            ignore_path
        ),
    )
    .unwrap();

    let default_violation = dir.path().join("default.yaml");
    fs::write(&default_violation, "---\n[1,2]\n").unwrap();

    let before_violation = dir.path().join("before-bad.yaml");
    fs::write(&before_violation, "---\n[1   , 2]\n").unwrap();

    let before_ok = dir.path().join("before-ok.yaml");
    fs::write(&before_ok, "---\n[1  , 2]\n").unwrap();

    let after_violation = dir.path().join("after-bad.yaml");
    fs::write(&after_violation, "---\n[1,    2]\n").unwrap();

    let after_ok = dir.path().join("after-ok.yaml");
    fs::write(&after_ok, "---\n[1,   2]\n").unwrap();

    let min_violation = dir.path().join("min-bad.yaml");
    fs::write(&min_violation, "---\n[1, 2]\n").unwrap();

    let min_ok = dir.path().join("min-ok.yaml");
    fs::write(&min_ok, "---\n[1,  2]\n").unwrap();

    let ignored_file = dir.path().join("ignored.yaml");
    fs::write(&ignored_file, "---\n[1,2]\n").unwrap();

    let ignored_from_file = dir.path().join("ignored-from-file.yaml");
    fs::write(&ignored_from_file, "---\n[1,2]\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_default
            .arg("-c")
            .arg(&default_cfg)
            .arg(&default_violation);
        let (ryl_default_code, ryl_default_output) = capture_with_env(ryl_default, scenario.envs);

        let mut yam_default = build_yamllint_command(scenario.yam_format);
        yam_default
            .arg("-c")
            .arg(&default_cfg)
            .arg(&default_violation);
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
        ryl_warning
            .arg("-c")
            .arg(&warning_cfg)
            .arg(&default_violation);
        let (ryl_warning_code, ryl_warning_output) = capture_with_env(ryl_warning, scenario.envs);

        let mut yam_warning = build_yamllint_command(scenario.yam_format);
        yam_warning
            .arg("-c")
            .arg(&warning_cfg)
            .arg(&default_violation);
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

        let mut ryl_before_bad = build_ryl_command(exe, scenario.ryl_format);
        ryl_before_bad
            .arg("-c")
            .arg(&before_cfg)
            .arg(&before_violation);
        let (ryl_before_bad_code, ryl_before_bad_output) =
            capture_with_env(ryl_before_bad, scenario.envs);

        let mut yam_before_bad = build_yamllint_command(scenario.yam_format);
        yam_before_bad
            .arg("-c")
            .arg(&before_cfg)
            .arg(&before_violation);
        let (yam_before_bad_code, yam_before_bad_output) =
            capture_with_env(yam_before_bad, scenario.envs);

        assert_eq!(
            ryl_before_bad_code, 1,
            "ryl before-bad exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_before_bad_code, 1,
            "yamllint before-bad exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_before_bad_output, yam_before_bad_output,
            "before-bad diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_before_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_before_ok.arg("-c").arg(&before_cfg).arg(&before_ok);
        let (ryl_before_ok_code, ryl_before_ok_output) =
            capture_with_env(ryl_before_ok, scenario.envs);

        let mut yam_before_ok = build_yamllint_command(scenario.yam_format);
        yam_before_ok.arg("-c").arg(&before_cfg).arg(&before_ok);
        let (yam_before_ok_code, yam_before_ok_output) =
            capture_with_env(yam_before_ok, scenario.envs);

        assert_eq!(
            ryl_before_ok_code, 0,
            "ryl before-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_before_ok_code, 0,
            "yamllint before-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_before_ok_output, yam_before_ok_output,
            "before-ok diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_after_bad = build_ryl_command(exe, scenario.ryl_format);
        ryl_after_bad
            .arg("-c")
            .arg(&after_cfg)
            .arg(&after_violation);
        let (ryl_after_bad_code, ryl_after_bad_output) =
            capture_with_env(ryl_after_bad, scenario.envs);

        let mut yam_after_bad = build_yamllint_command(scenario.yam_format);
        yam_after_bad
            .arg("-c")
            .arg(&after_cfg)
            .arg(&after_violation);
        let (yam_after_bad_code, yam_after_bad_output) =
            capture_with_env(yam_after_bad, scenario.envs);

        assert_eq!(
            ryl_after_bad_code, 1,
            "ryl after-bad exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_after_bad_code, 1,
            "yamllint after-bad exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_after_bad_output, yam_after_bad_output,
            "after-bad diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_after_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_after_ok.arg("-c").arg(&after_cfg).arg(&after_ok);
        let (ryl_after_ok_code, ryl_after_ok_output) =
            capture_with_env(ryl_after_ok, scenario.envs);

        let mut yam_after_ok = build_yamllint_command(scenario.yam_format);
        yam_after_ok.arg("-c").arg(&after_cfg).arg(&after_ok);
        let (yam_after_ok_code, yam_after_ok_output) =
            capture_with_env(yam_after_ok, scenario.envs);

        assert_eq!(
            ryl_after_ok_code, 0,
            "ryl after-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_after_ok_code, 0,
            "yamllint after-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_after_ok_output, yam_after_ok_output,
            "after-ok diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_min_bad = build_ryl_command(exe, scenario.ryl_format);
        ryl_min_bad.arg("-c").arg(&min_cfg).arg(&min_violation);
        let (ryl_min_bad_code, ryl_min_bad_output) = capture_with_env(ryl_min_bad, scenario.envs);

        let mut yam_min_bad = build_yamllint_command(scenario.yam_format);
        yam_min_bad.arg("-c").arg(&min_cfg).arg(&min_violation);
        let (yam_min_bad_code, yam_min_bad_output) = capture_with_env(yam_min_bad, scenario.envs);

        assert_eq!(ryl_min_bad_code, 1, "ryl min-bad exit ({})", scenario.label);
        assert_eq!(
            yam_min_bad_code, 1,
            "yamllint min-bad exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_min_bad_output, yam_min_bad_output,
            "min-bad diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_min_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_min_ok.arg("-c").arg(&min_cfg).arg(&min_ok);
        let (ryl_min_ok_code, ryl_min_ok_output) = capture_with_env(ryl_min_ok, scenario.envs);

        let mut yam_min_ok = build_yamllint_command(scenario.yam_format);
        yam_min_ok.arg("-c").arg(&min_cfg).arg(&min_ok);
        let (yam_min_ok_code, yam_min_ok_output) = capture_with_env(yam_min_ok, scenario.envs);

        assert_eq!(ryl_min_ok_code, 0, "ryl min-ok exit ({})", scenario.label);
        assert_eq!(
            yam_min_ok_code, 0,
            "yamllint min-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_min_ok_output, yam_min_ok_output,
            "min-ok diagnostics mismatch ({})",
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
