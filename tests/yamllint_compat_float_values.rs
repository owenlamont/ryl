use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn float_values_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let violations = dir.path().join("violations.yaml");
    fs::write(&violations, "a: .5\nb: 1e2\nc: .nan\nd: .inf\n").unwrap();

    let ok = dir.path().join("ok.yaml");
    fs::write(&ok, "a: 0.5\nb: 10.0\nc: 0.0\n").unwrap();

    let default_cfg = dir.path().join("float-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  float-values: enable\n",
    )
    .unwrap();

    let require_cfg = dir.path().join("float-require.yml");
    fs::write(
        &require_cfg,
        "rules:\n  document-start: disable\n  float-values:\n    require-numeral-before-decimal: true\n",
    )
    .unwrap();

    let scientific_cfg = dir.path().join("float-scientific.yml");
    fs::write(
        &scientific_cfg,
        "rules:\n  document-start: disable\n  float-values:\n    forbid-scientific-notation: true\n",
    )
    .unwrap();

    let nan_cfg = dir.path().join("float-nan.yml");
    fs::write(
        &nan_cfg,
        "rules:\n  document-start: disable\n  float-values:\n    forbid-nan: true\n",
    )
    .unwrap();

    let inf_cfg = dir.path().join("float-inf.yml");
    fs::write(
        &inf_cfg,
        "rules:\n  document-start: disable\n  float-values:\n    forbid-inf: true\n",
    )
    .unwrap();

    let all_cfg = dir.path().join("float-all.yml");
    fs::write(
        &all_cfg,
        "rules:\n  document-start: disable\n  float-values:\n    require-numeral-before-decimal: true\n    forbid-scientific-notation: true\n    forbid-nan: true\n    forbid-inf: true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        // Default configuration should not flag anything
        let mut ryl_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_default.arg("-c").arg(&default_cfg).arg(&violations);
        let (ryl_default_code, ryl_default_msg) = capture_with_env(ryl_default, scenario.envs);

        let mut yam_default = build_yamllint_command(scenario.yam_format);
        yam_default.arg("-c").arg(&default_cfg).arg(&violations);
        let (yam_default_code, yam_default_msg) = capture_with_env(yam_default, scenario.envs);

        assert_eq!(ryl_default_code, 0, "ryl default exit ({})", scenario.label);
        assert_eq!(
            yam_default_code, 0,
            "yamllint default exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_default_msg, yam_default_msg,
            "default diagnostics mismatch ({})",
            scenario.label
        );

        // require-numeral-before-decimal
        let mut ryl_require = build_ryl_command(exe, scenario.ryl_format);
        ryl_require.arg("-c").arg(&require_cfg).arg(&violations);
        let (ryl_require_code, ryl_require_msg) = capture_with_env(ryl_require, scenario.envs);

        let mut yam_require = build_yamllint_command(scenario.yam_format);
        yam_require.arg("-c").arg(&require_cfg).arg(&violations);
        let (yam_require_code, yam_require_msg) = capture_with_env(yam_require, scenario.envs);

        assert_eq!(ryl_require_code, 1, "ryl require exit ({})", scenario.label);
        assert_eq!(
            yam_require_code, 1,
            "yamllint require exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_require_msg, yam_require_msg,
            "require diagnostics mismatch ({})",
            scenario.label
        );

        // forbid-scientific-notation
        let mut ryl_scientific = build_ryl_command(exe, scenario.ryl_format);
        ryl_scientific
            .arg("-c")
            .arg(&scientific_cfg)
            .arg(&violations);
        let (ryl_scientific_code, ryl_scientific_msg) =
            capture_with_env(ryl_scientific, scenario.envs);

        let mut yam_scientific = build_yamllint_command(scenario.yam_format);
        yam_scientific
            .arg("-c")
            .arg(&scientific_cfg)
            .arg(&violations);
        let (yam_scientific_code, yam_scientific_msg) =
            capture_with_env(yam_scientific, scenario.envs);

        assert_eq!(
            ryl_scientific_code, 1,
            "ryl scientific exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_scientific_code, 1,
            "yamllint scientific exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_scientific_msg, yam_scientific_msg,
            "scientific diagnostics mismatch ({})",
            scenario.label
        );

        // forbid-nan
        let mut ryl_nan = build_ryl_command(exe, scenario.ryl_format);
        ryl_nan.arg("-c").arg(&nan_cfg).arg(&violations);
        let (ryl_nan_code, ryl_nan_msg) = capture_with_env(ryl_nan, scenario.envs);

        let mut yam_nan = build_yamllint_command(scenario.yam_format);
        yam_nan.arg("-c").arg(&nan_cfg).arg(&violations);
        let (yam_nan_code, yam_nan_msg) = capture_with_env(yam_nan, scenario.envs);

        assert_eq!(ryl_nan_code, 1, "ryl nan exit ({})", scenario.label);
        assert_eq!(yam_nan_code, 1, "yamllint nan exit ({})", scenario.label);
        assert_eq!(
            ryl_nan_msg, yam_nan_msg,
            "nan diagnostics mismatch ({})",
            scenario.label
        );

        // forbid-inf
        let mut ryl_inf = build_ryl_command(exe, scenario.ryl_format);
        ryl_inf.arg("-c").arg(&inf_cfg).arg(&violations);
        let (ryl_inf_code, ryl_inf_msg) = capture_with_env(ryl_inf, scenario.envs);

        let mut yam_inf = build_yamllint_command(scenario.yam_format);
        yam_inf.arg("-c").arg(&inf_cfg).arg(&violations);
        let (yam_inf_code, yam_inf_msg) = capture_with_env(yam_inf, scenario.envs);

        assert_eq!(ryl_inf_code, 1, "ryl inf exit ({})", scenario.label);
        assert_eq!(yam_inf_code, 1, "yamllint inf exit ({})", scenario.label);
        assert_eq!(
            ryl_inf_msg, yam_inf_msg,
            "inf diagnostics mismatch ({})",
            scenario.label
        );

        // all options enabled, expect four diagnostics, and ok file passes
        let mut ryl_all = build_ryl_command(exe, scenario.ryl_format);
        ryl_all.arg("-c").arg(&all_cfg).arg(&violations);
        let (ryl_all_code, ryl_all_msg) = capture_with_env(ryl_all, scenario.envs);

        let mut yam_all = build_yamllint_command(scenario.yam_format);
        yam_all.arg("-c").arg(&all_cfg).arg(&violations);
        let (yam_all_code, yam_all_msg) = capture_with_env(yam_all, scenario.envs);

        assert_eq!(ryl_all_code, 1, "ryl all exit ({})", scenario.label);
        assert_eq!(yam_all_code, 1, "yamllint all exit ({})", scenario.label);
        assert_eq!(
            ryl_all_msg, yam_all_msg,
            "all diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_ok.arg("-c").arg(&all_cfg).arg(&ok);
        let (ryl_ok_code, ryl_ok_msg) = capture_with_env(ryl_ok, scenario.envs);

        let mut yam_ok = build_yamllint_command(scenario.yam_format);
        yam_ok.arg("-c").arg(&all_cfg).arg(&ok);
        let (yam_ok_code, yam_ok_msg) = capture_with_env(yam_ok, scenario.envs);

        assert_eq!(ryl_ok_code, 0, "ryl ok exit ({})", scenario.label);
        assert_eq!(yam_ok_code, 0, "yamllint ok exit ({})", scenario.label);
        assert_eq!(
            ryl_ok_msg, yam_ok_msg,
            "ok diagnostics mismatch ({})",
            scenario.label
        );
    }
}
