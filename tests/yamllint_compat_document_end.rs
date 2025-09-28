use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn document_end_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let missing = dir.path().join("missing.yaml");
    fs::write(&missing, "---\nfoo: bar\n").unwrap();

    let explicit = dir.path().join("explicit.yaml");
    fs::write(&explicit, "---\nfoo: bar\n...\n").unwrap();

    let require_cfg = dir.path().join("require.yml");
    fs::write(
        &require_cfg,
        "rules:\n  document-end:\n    level: error\n    present: true\n",
    )
    .unwrap();

    let forbid_cfg = dir.path().join("forbid.yml");
    fs::write(
        &forbid_cfg,
        "rules:\n  document-end:\n    level: error\n    present: false\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_require = build_ryl_command(exe, scenario.ryl_format);
        ryl_require.arg("-c").arg(&require_cfg).arg(&missing);
        let (ryl_req_code, ryl_req_msg) = capture_with_env(ryl_require, scenario.envs);

        let mut yam_require = build_yamllint_command(scenario.yam_format);
        yam_require.arg("-c").arg(&require_cfg).arg(&missing);
        let (yam_req_code, yam_req_msg) = capture_with_env(yam_require, scenario.envs);

        assert_eq!(ryl_req_code, 1, "ryl require exit ({})", scenario.label);
        assert_eq!(
            yam_req_code, 1,
            "yamllint require exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_req_msg, yam_req_msg,
            "missing marker diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_require_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_require_ok.arg("-c").arg(&require_cfg).arg(&explicit);
        let (ryl_req_ok_code, ryl_req_ok_msg) = capture_with_env(ryl_require_ok, scenario.envs);

        let mut yam_require_ok = build_yamllint_command(scenario.yam_format);
        yam_require_ok.arg("-c").arg(&require_cfg).arg(&explicit);
        let (yam_req_ok_code, yam_req_ok_msg) = capture_with_env(yam_require_ok, scenario.envs);

        assert_eq!(
            ryl_req_ok_code, 0,
            "ryl require pass exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_req_ok_code, 0,
            "yamllint require pass exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_req_ok_msg, yam_req_ok_msg,
            "require pass diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_forbid = build_ryl_command(exe, scenario.ryl_format);
        ryl_forbid.arg("-c").arg(&forbid_cfg).arg(&explicit);
        let (ryl_forbid_code, ryl_forbid_msg) = capture_with_env(ryl_forbid, scenario.envs);

        let mut yam_forbid = build_yamllint_command(scenario.yam_format);
        yam_forbid.arg("-c").arg(&forbid_cfg).arg(&explicit);
        let (yam_forbid_code, yam_forbid_msg) = capture_with_env(yam_forbid, scenario.envs);

        assert_eq!(ryl_forbid_code, 1, "ryl forbid exit ({})", scenario.label);
        assert_eq!(
            yam_forbid_code, 1,
            "yamllint forbid exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_forbid_msg, yam_forbid_msg,
            "forbid diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_forbid_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_forbid_ok.arg("-c").arg(&forbid_cfg).arg(&missing);
        let (ryl_forbid_ok_code, ryl_forbid_ok_msg) =
            capture_with_env(ryl_forbid_ok, scenario.envs);

        let mut yam_forbid_ok = build_yamllint_command(scenario.yam_format);
        yam_forbid_ok.arg("-c").arg(&forbid_cfg).arg(&missing);
        let (yam_forbid_ok_code, yam_forbid_ok_msg) =
            capture_with_env(yam_forbid_ok, scenario.envs);

        assert_eq!(
            ryl_forbid_ok_code, 0,
            "ryl forbid pass exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_forbid_ok_code, 0,
            "yamllint forbid pass exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_forbid_ok_msg, yam_forbid_ok_msg,
            "forbid pass diagnostics mismatch ({})",
            scenario.label
        );
    }
}
