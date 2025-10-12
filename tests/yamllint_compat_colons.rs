use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn colons_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("colons-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  colons: enable\n",
    )
    .unwrap();

    let warning_cfg = dir.path().join("colons-warning.yml");
    fs::write(
        &warning_cfg,
        "rules:\n  document-start: disable\n  colons:\n    level: warning\n",
    )
    .unwrap();

    let before_cfg = dir.path().join("colons-before.yml");
    fs::write(
        &before_cfg,
        "rules:\n  document-start: disable\n  colons:\n    max-spaces-before: 2\n",
    )
    .unwrap();

    let after_cfg = dir.path().join("colons-after.yml");
    fs::write(
        &after_cfg,
        "rules:\n  document-start: disable\n  colons:\n    max-spaces-after: 3\n",
    )
    .unwrap();

    let default_violation = dir.path().join("default.yaml");
    fs::write(&default_violation, "---\nkey :  value\n").unwrap();

    let warning_violation = dir.path().join("warning.yaml");
    fs::write(&warning_violation, "---\nkey :  value\n").unwrap();

    let before_bad = dir.path().join("before-bad.yaml");
    fs::write(&before_bad, "---\nkey   : value\n").unwrap();

    let before_ok = dir.path().join("before-ok.yaml");
    fs::write(&before_ok, "---\nkey  : value\n").unwrap();

    let after_bad = dir.path().join("after-bad.yaml");
    fs::write(&after_bad, "---\nkey:    value\n").unwrap();

    let after_ok = dir.path().join("after-ok.yaml");
    fs::write(&after_ok, "---\nkey:   value\n").unwrap();

    let question_bad = dir.path().join("question-bad.yaml");
    fs::write(&question_bad, "---\n?  key\n: value\n").unwrap();

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
            .arg(&warning_violation);
        let (ryl_warning_code, ryl_warning_output) = capture_with_env(ryl_warning, scenario.envs);

        let mut yam_warning = build_yamllint_command(scenario.yam_format);
        yam_warning
            .arg("-c")
            .arg(&warning_cfg)
            .arg(&warning_violation);
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
        ryl_before_bad.arg("-c").arg(&before_cfg).arg(&before_bad);
        let (ryl_before_bad_code, ryl_before_bad_output) =
            capture_with_env(ryl_before_bad, scenario.envs);

        let mut yam_before_bad = build_yamllint_command(scenario.yam_format);
        yam_before_bad.arg("-c").arg(&before_cfg).arg(&before_bad);
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
        ryl_after_bad.arg("-c").arg(&after_cfg).arg(&after_bad);
        let (ryl_after_bad_code, ryl_after_bad_output) =
            capture_with_env(ryl_after_bad, scenario.envs);

        let mut yam_after_bad = build_yamllint_command(scenario.yam_format);
        yam_after_bad.arg("-c").arg(&after_cfg).arg(&after_bad);
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

        let mut ryl_question = build_ryl_command(exe, scenario.ryl_format);
        ryl_question.arg("-c").arg(&default_cfg).arg(&question_bad);
        let (ryl_question_code, ryl_question_output) =
            capture_with_env(ryl_question, scenario.envs);

        let mut yam_question = build_yamllint_command(scenario.yam_format);
        yam_question.arg("-c").arg(&default_cfg).arg(&question_bad);
        let (yam_question_code, yam_question_output) =
            capture_with_env(yam_question, scenario.envs);

        assert_eq!(
            ryl_question_code, 1,
            "ryl question exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_question_code, 1,
            "yamllint question exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_question_output, yam_question_output,
            "question diagnostics mismatch ({})",
            scenario.label
        );
    }
}

#[test]
fn inline_comment_after_value_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let cfg_path = dir.path().join("colons.yaml");
    fs::write(&cfg_path, "rules:\n  document-start: disable\n").unwrap();

    let yaml_path = dir.path().join("inline.yaml");
    fs::write(
        &yaml_path,
        "---\n- debug:  # noqa comment\n    var: value\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (ryl_code, ryl_output) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (yam_code, yam_output) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(ryl_code, 0, "ryl emitted diagnostics ({})", scenario.label);
        assert_eq!(
            yam_code, 0,
            "yamllint emitted diagnostics ({})",
            scenario.label
        );
        assert_eq!(
            ryl_output, yam_output,
            "output mismatch ({})",
            scenario.label
        );
    }
}
