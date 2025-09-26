use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn octal_values_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("octal-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  octal-values: enable\n",
    )
    .unwrap();

    let implicit_ok_cfg = dir.path().join("octal-implicit-ok.yml");
    fs::write(
        &implicit_ok_cfg,
        "rules:\n  document-start: disable\n  octal-values:\n    forbid-implicit-octal: false\n",
    )
    .unwrap();

    let explicit_ok_cfg = dir.path().join("octal-explicit-ok.yml");
    fs::write(
        &explicit_ok_cfg,
        "rules:\n  document-start: disable\n  octal-values:\n    forbid-explicit-octal: false\n",
    )
    .unwrap();

    let all_allowed_cfg = dir.path().join("octal-all-allowed.yml");
    fs::write(
        &all_allowed_cfg,
        "rules:\n  document-start: disable\n  octal-values:\n    forbid-implicit-octal: false\n    forbid-explicit-octal: false\n",
    )
    .unwrap();

    let bad_file = dir.path().join("bad.yaml");
    fs::write(&bad_file, "foo: 010\nbar: 0o10\n").unwrap();

    let ok_file = dir.path().join("ok.yaml");
    fs::write(&ok_file, "foo: '010'\nbar: 0O10\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_default.arg("-c").arg(&default_cfg).arg(&bad_file);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_default, scenario.envs);

        let mut yam_default = build_yamllint_command(scenario.yam_format);
        yam_default.arg("-c").arg(&default_cfg).arg(&bad_file);
        let (yam_code, yam_msg) = capture_with_env(yam_default, scenario.envs);

        assert_eq!(ryl_code, 1, "ryl default exit ({})", scenario.label);
        assert_eq!(yam_code, 1, "yamllint default exit ({})", scenario.label);
        assert_eq!(
            ryl_msg, yam_msg,
            "default diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_implicit_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_implicit_ok
            .arg("-c")
            .arg(&implicit_ok_cfg)
            .arg(&bad_file);
        let (ryl_imp_code, ryl_imp_msg) = capture_with_env(ryl_implicit_ok, scenario.envs);

        let mut yam_implicit_ok = build_yamllint_command(scenario.yam_format);
        yam_implicit_ok
            .arg("-c")
            .arg(&implicit_ok_cfg)
            .arg(&bad_file);
        let (yam_imp_code, yam_imp_msg) = capture_with_env(yam_implicit_ok, scenario.envs);

        assert_eq!(ryl_imp_code, 1, "ryl implicit-ok exit ({})", scenario.label);
        assert_eq!(
            yam_imp_code, 1,
            "yamllint implicit-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_imp_msg, yam_imp_msg,
            "implicit-ok diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_explicit_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_explicit_ok
            .arg("-c")
            .arg(&explicit_ok_cfg)
            .arg(&bad_file);
        let (ryl_exp_code, ryl_exp_msg) = capture_with_env(ryl_explicit_ok, scenario.envs);

        let mut yam_explicit_ok = build_yamllint_command(scenario.yam_format);
        yam_explicit_ok
            .arg("-c")
            .arg(&explicit_ok_cfg)
            .arg(&bad_file);
        let (yam_exp_code, yam_exp_msg) = capture_with_env(yam_explicit_ok, scenario.envs);

        assert_eq!(ryl_exp_code, 1, "ryl explicit-ok exit ({})", scenario.label);
        assert_eq!(
            yam_exp_code, 1,
            "yamllint explicit-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_exp_msg, yam_exp_msg,
            "explicit-ok diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_all_allowed = build_ryl_command(exe, scenario.ryl_format);
        ryl_all_allowed
            .arg("-c")
            .arg(&all_allowed_cfg)
            .arg(&bad_file);
        let (ryl_all_code, ryl_all_msg) = capture_with_env(ryl_all_allowed, scenario.envs);

        let mut yam_all_allowed = build_yamllint_command(scenario.yam_format);
        yam_all_allowed
            .arg("-c")
            .arg(&all_allowed_cfg)
            .arg(&bad_file);
        let (yam_all_code, yam_all_msg) = capture_with_env(yam_all_allowed, scenario.envs);

        assert_eq!(ryl_all_code, 0, "ryl all-allowed exit ({})", scenario.label);
        assert_eq!(
            yam_all_code, 0,
            "yamllint all-allowed exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_all_msg, yam_all_msg,
            "all-allowed diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_ok.arg("-c").arg(&default_cfg).arg(&ok_file);
        let (ryl_ok_code, ryl_ok_msg) = capture_with_env(ryl_ok, scenario.envs);

        let mut yam_ok = build_yamllint_command(scenario.yam_format);
        yam_ok.arg("-c").arg(&default_cfg).arg(&ok_file);
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
