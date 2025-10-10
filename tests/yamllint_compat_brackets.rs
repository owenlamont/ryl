use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn brackets_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("brackets-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  brackets: enable\n",
    )
    .unwrap();

    let warning_cfg = dir.path().join("brackets-warning.yml");
    fs::write(
        &warning_cfg,
        "rules:\n  document-start: disable\n  brackets:\n    level: warning\n",
    )
    .unwrap();

    let min_cfg = dir.path().join("brackets-min.yml");
    fs::write(
        &min_cfg,
        "rules:\n  document-start: disable\n  brackets:\n    min-spaces-inside: 1\n    max-spaces-inside: -1\n",
    )
    .unwrap();

    let max_cfg = dir.path().join("brackets-max.yml");
    fs::write(
        &max_cfg,
        "rules:\n  document-start: disable\n  brackets:\n    max-spaces-inside: 1\n",
    )
    .unwrap();

    let empty_cfg = dir.path().join("brackets-empty.yml");
    fs::write(
        &empty_cfg,
        "rules:\n  document-start: disable\n  brackets:\n    min-spaces-inside-empty: 1\n    max-spaces-inside-empty: 2\n",
    )
    .unwrap();

    let forbid_cfg = dir.path().join("brackets-forbid.yml");
    fs::write(
        &forbid_cfg,
        "rules:\n  document-start: disable\n  brackets:\n    forbid: true\n",
    )
    .unwrap();

    let forbid_non_empty_cfg = dir.path().join("brackets-forbid-non-empty.yml");
    fs::write(
        &forbid_non_empty_cfg,
        "rules:\n  document-start: disable\n  brackets:\n    forbid: non-empty\n",
    )
    .unwrap();

    let default_violation = dir.path().join("default.yaml");
    fs::write(&default_violation, "---\nobject: [ 1, 2 ]\n").unwrap();

    let min_violation = dir.path().join("min.yaml");
    fs::write(&min_violation, "---\nobject: [1, 2]\n").unwrap();

    let max_violation = dir.path().join("max.yaml");
    fs::write(&max_violation, "---\nobject: [  1, 2   ]\n").unwrap();

    let empty_min_violation = dir.path().join("empty-min.yaml");
    fs::write(&empty_min_violation, "---\nobject: []\n").unwrap();

    let empty_max_violation = dir.path().join("empty-max.yaml");
    fs::write(&empty_max_violation, "---\nobject: [    ]\n").unwrap();

    let forbid_violation = dir.path().join("forbid.yaml");
    fs::write(&forbid_violation, "---\nobject: [1, 2]\n").unwrap();

    let forbid_spaces_violation = dir.path().join("forbid-spaces.yaml");
    fs::write(&forbid_spaces_violation, "---\nobject: [ 1, 2 ]\n").unwrap();

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

        let mut ryl_min = build_ryl_command(exe, scenario.ryl_format);
        ryl_min.arg("-c").arg(&min_cfg).arg(&min_violation);
        let (ryl_min_code, ryl_min_output) = capture_with_env(ryl_min, scenario.envs);

        let mut yam_min = build_yamllint_command(scenario.yam_format);
        yam_min.arg("-c").arg(&min_cfg).arg(&min_violation);
        let (yam_min_code, yam_min_output) = capture_with_env(yam_min, scenario.envs);

        assert_eq!(ryl_min_code, 1, "ryl min exit ({})", scenario.label);
        assert_eq!(yam_min_code, 1, "yamllint min exit ({})", scenario.label);
        assert_eq!(
            ryl_min_output, yam_min_output,
            "min diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_max = build_ryl_command(exe, scenario.ryl_format);
        ryl_max.arg("-c").arg(&max_cfg).arg(&max_violation);
        let (ryl_max_code, ryl_max_output) = capture_with_env(ryl_max, scenario.envs);

        let mut yam_max = build_yamllint_command(scenario.yam_format);
        yam_max.arg("-c").arg(&max_cfg).arg(&max_violation);
        let (yam_max_code, yam_max_output) = capture_with_env(yam_max, scenario.envs);

        assert_eq!(ryl_max_code, 1, "ryl max exit ({})", scenario.label);
        assert_eq!(yam_max_code, 1, "yamllint max exit ({})", scenario.label);
        assert_eq!(
            ryl_max_output, yam_max_output,
            "max diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_empty_min = build_ryl_command(exe, scenario.ryl_format);
        ryl_empty_min
            .arg("-c")
            .arg(&empty_cfg)
            .arg(&empty_min_violation);
        let (ryl_empty_min_code, ryl_empty_min_output) =
            capture_with_env(ryl_empty_min, scenario.envs);

        let mut yam_empty_min = build_yamllint_command(scenario.yam_format);
        yam_empty_min
            .arg("-c")
            .arg(&empty_cfg)
            .arg(&empty_min_violation);
        let (yam_empty_min_code, yam_empty_min_output) =
            capture_with_env(yam_empty_min, scenario.envs);

        assert_eq!(
            ryl_empty_min_code, 1,
            "ryl empty-min exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_empty_min_code, 1,
            "yamllint empty-min exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_empty_min_output, yam_empty_min_output,
            "empty-min diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_empty_max = build_ryl_command(exe, scenario.ryl_format);
        ryl_empty_max
            .arg("-c")
            .arg(&empty_cfg)
            .arg(&empty_max_violation);
        let (ryl_empty_max_code, ryl_empty_max_output) =
            capture_with_env(ryl_empty_max, scenario.envs);

        let mut yam_empty_max = build_yamllint_command(scenario.yam_format);
        yam_empty_max
            .arg("-c")
            .arg(&empty_cfg)
            .arg(&empty_max_violation);
        let (yam_empty_max_code, yam_empty_max_output) =
            capture_with_env(yam_empty_max, scenario.envs);

        assert_eq!(
            ryl_empty_max_code, 1,
            "ryl empty-max exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_empty_max_code, 1,
            "yamllint empty-max exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_empty_max_output, yam_empty_max_output,
            "empty-max diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_forbid = build_ryl_command(exe, scenario.ryl_format);
        ryl_forbid.arg("-c").arg(&forbid_cfg).arg(&forbid_violation);
        let (ryl_forbid_code, ryl_forbid_output) = capture_with_env(ryl_forbid, scenario.envs);

        let mut yam_forbid = build_yamllint_command(scenario.yam_format);
        yam_forbid.arg("-c").arg(&forbid_cfg).arg(&forbid_violation);
        let (yam_forbid_code, yam_forbid_output) = capture_with_env(yam_forbid, scenario.envs);

        assert_eq!(ryl_forbid_code, 1, "ryl forbid exit ({})", scenario.label);
        assert_eq!(
            yam_forbid_code, 1,
            "yamllint forbid exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_forbid_output, yam_forbid_output,
            "forbid diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_forbid_spaces = build_ryl_command(exe, scenario.ryl_format);
        ryl_forbid_spaces
            .arg("-c")
            .arg(&forbid_cfg)
            .arg(&forbid_spaces_violation);
        let (ryl_forbid_spaces_code, ryl_forbid_spaces_output) =
            capture_with_env(ryl_forbid_spaces, scenario.envs);

        let mut yam_forbid_spaces = build_yamllint_command(scenario.yam_format);
        yam_forbid_spaces
            .arg("-c")
            .arg(&forbid_cfg)
            .arg(&forbid_spaces_violation);
        let (yam_forbid_spaces_code, yam_forbid_spaces_output) =
            capture_with_env(yam_forbid_spaces, scenario.envs);

        assert_eq!(
            ryl_forbid_spaces_code, 1,
            "ryl forbid-spaces exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_forbid_spaces_code, 1,
            "yamllint forbid-spaces exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_forbid_spaces_output, yam_forbid_spaces_output,
            "forbid-spaces diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_forbid_non_empty = build_ryl_command(exe, scenario.ryl_format);
        ryl_forbid_non_empty
            .arg("-c")
            .arg(&forbid_non_empty_cfg)
            .arg(&forbid_violation);
        let (ryl_forbid_non_empty_code, ryl_forbid_non_empty_output) =
            capture_with_env(ryl_forbid_non_empty, scenario.envs);

        let mut yam_forbid_non_empty = build_yamllint_command(scenario.yam_format);
        yam_forbid_non_empty
            .arg("-c")
            .arg(&forbid_non_empty_cfg)
            .arg(&forbid_violation);
        let (yam_forbid_non_empty_code, yam_forbid_non_empty_output) =
            capture_with_env(yam_forbid_non_empty, scenario.envs);

        assert_eq!(
            ryl_forbid_non_empty_code, 1,
            "ryl forbid-non-empty exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_forbid_non_empty_code, 1,
            "yamllint forbid-non-empty exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_forbid_non_empty_output, yam_forbid_non_empty_output,
            "forbid-non-empty diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_forbid_non_empty_spaces = build_ryl_command(exe, scenario.ryl_format);
        ryl_forbid_non_empty_spaces
            .arg("-c")
            .arg(&forbid_non_empty_cfg)
            .arg(&forbid_spaces_violation);
        let (ryl_forbid_non_empty_spaces_code, ryl_forbid_non_empty_spaces_output) =
            capture_with_env(ryl_forbid_non_empty_spaces, scenario.envs);

        let mut yam_forbid_non_empty_spaces = build_yamllint_command(scenario.yam_format);
        yam_forbid_non_empty_spaces
            .arg("-c")
            .arg(&forbid_non_empty_cfg)
            .arg(&forbid_spaces_violation);
        let (yam_forbid_non_empty_spaces_code, yam_forbid_non_empty_spaces_output) =
            capture_with_env(yam_forbid_non_empty_spaces, scenario.envs);

        assert_eq!(
            ryl_forbid_non_empty_spaces_code, 1,
            "ryl forbid-non-empty-spaces exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_forbid_non_empty_spaces_code, 1,
            "yamllint forbid-non-empty-spaces exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_forbid_non_empty_spaces_output, yam_forbid_non_empty_spaces_output,
            "forbid-non-empty-spaces diagnostics mismatch ({})",
            scenario.label
        );
    }
}
