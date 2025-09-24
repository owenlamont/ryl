use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn truthy_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("truthy-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  truthy: enable\n",
    )
    .unwrap();

    let allowed_cfg = dir.path().join("truthy-allowed.yml");
    fs::write(
        &allowed_cfg,
        "rules:\n  document-start: disable\n  truthy:\n    allowed-values: [\"yes\", \"no\"]\n",
    )
    .unwrap();

    let check_keys_cfg = dir.path().join("truthy-check-keys.yml");
    fs::write(
        &check_keys_cfg,
        "rules:\n  document-start: disable\n  truthy:\n    allowed-values: []\n    check-keys: false\n",
    )
    .unwrap();

    let bad_file = dir.path().join("bad.yaml");
    fs::write(&bad_file, "foo: True\nbar: yes\nTrue: 1\non: off\n").unwrap();

    let allowed_file = dir.path().join("allowed.yaml");
    fs::write(&allowed_file, "- yes\n- no\n- true\n- on\n").unwrap();

    let ok_file = dir.path().join("ok.yaml");
    fs::write(&ok_file, "foo: false\nbar: true\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        // default configuration, expect errors on keys and values
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

        // allowed-values override keeps yes/no allowed
        let mut ryl_allowed = build_ryl_command(exe, scenario.ryl_format);
        ryl_allowed.arg("-c").arg(&allowed_cfg).arg(&allowed_file);
        let (ryl_allowed_code, ryl_allowed_msg) = capture_with_env(ryl_allowed, scenario.envs);

        let mut yam_allowed = build_yamllint_command(scenario.yam_format);
        yam_allowed.arg("-c").arg(&allowed_cfg).arg(&allowed_file);
        let (yam_allowed_code, yam_allowed_msg) = capture_with_env(yam_allowed, scenario.envs);

        assert_eq!(ryl_allowed_code, 1, "ryl allowed exit ({})", scenario.label);
        assert_eq!(
            yam_allowed_code, 1,
            "yamllint allowed exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_allowed_msg, yam_allowed_msg,
            "allowed diagnostics mismatch ({})",
            scenario.label
        );

        // check-keys=false should suppress key diagnostics
        let mut ryl_check_keys = build_ryl_command(exe, scenario.ryl_format);
        ryl_check_keys.arg("-c").arg(&check_keys_cfg).arg(&bad_file);
        let (ryl_ck_code, ryl_ck_msg) = capture_with_env(ryl_check_keys, scenario.envs);

        let mut yam_check_keys = build_yamllint_command(scenario.yam_format);
        yam_check_keys.arg("-c").arg(&check_keys_cfg).arg(&bad_file);
        let (yam_ck_code, yam_ck_msg) = capture_with_env(yam_check_keys, scenario.envs);

        assert_eq!(ryl_ck_code, 1, "ryl check-keys exit ({})", scenario.label);
        assert_eq!(
            yam_ck_code, 1,
            "yamllint check-keys exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_ck_msg, yam_ck_msg,
            "check-keys diagnostics mismatch ({})",
            scenario.label
        );

        // success scenario: default config but compliant file
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
