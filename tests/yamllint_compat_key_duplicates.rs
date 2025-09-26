use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn key_duplicates_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("key-duplicates-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  key-duplicates: enable\n",
    )
    .unwrap();

    let forbid_cfg = dir.path().join("key-duplicates-forbid.yml");
    fs::write(
        &forbid_cfg,
        "rules:\n  document-start: disable\n  key-duplicates:\n    forbid-duplicated-merge-keys: true\n",
    )
    .unwrap();

    let dup_bad = dir.path().join("dup-bad.yaml");
    fs::write(&dup_bad, "foo: 1\nbar: 2\nfoo: 3\n").unwrap();

    let dup_ok = dir.path().join("dup-ok.yaml");
    fs::write(&dup_ok, "foo: 1\nbar: 2\n").unwrap();

    let merge_dup = dir.path().join("merge-dup.yaml");
    fs::write(
        &merge_dup,
        "anchor: &a\n  value: 1\nmerged:\n  <<: *a\n  <<: *a\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_bad = build_ryl_command(exe, scenario.ryl_format);
        ryl_bad.arg("-c").arg(&default_cfg).arg(&dup_bad);
        let (ryl_bad_code, ryl_bad_msg) = capture_with_env(ryl_bad, scenario.envs);

        let mut yam_bad = build_yamllint_command(scenario.yam_format);
        yam_bad.arg("-c").arg(&default_cfg).arg(&dup_bad);
        let (yam_bad_code, yam_bad_msg) = capture_with_env(yam_bad, scenario.envs);

        assert_eq!(ryl_bad_code, 1, "ryl dup exit ({})", scenario.label);
        assert_eq!(yam_bad_code, 1, "yamllint dup exit ({})", scenario.label);
        assert_eq!(
            ryl_bad_msg, yam_bad_msg,
            "duplicate diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_ok.arg("-c").arg(&default_cfg).arg(&dup_ok);
        let (ryl_ok_code, ryl_ok_msg) = capture_with_env(ryl_ok, scenario.envs);

        let mut yam_ok = build_yamllint_command(scenario.yam_format);
        yam_ok.arg("-c").arg(&default_cfg).arg(&dup_ok);
        let (yam_ok_code, yam_ok_msg) = capture_with_env(yam_ok, scenario.envs);

        assert_eq!(ryl_ok_code, 0, "ryl ok exit ({})", scenario.label);
        assert_eq!(yam_ok_code, 0, "yamllint ok exit ({})", scenario.label);
        assert_eq!(
            ryl_ok_msg, yam_ok_msg,
            "ok diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_merge_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_merge_default
            .arg("-c")
            .arg(&default_cfg)
            .arg(&merge_dup);
        let (ryl_merge_def_code, ryl_merge_def_msg) =
            capture_with_env(ryl_merge_default, scenario.envs);

        let mut yam_merge_default = build_yamllint_command(scenario.yam_format);
        yam_merge_default
            .arg("-c")
            .arg(&default_cfg)
            .arg(&merge_dup);
        let (yam_merge_def_code, yam_merge_def_msg) =
            capture_with_env(yam_merge_default, scenario.envs);

        assert_eq!(
            ryl_merge_def_code, 0,
            "ryl merge default exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_merge_def_code, 0,
            "yamllint merge default exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_merge_def_msg, yam_merge_def_msg,
            "merge default diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_merge_forbid = build_ryl_command(exe, scenario.ryl_format);
        ryl_merge_forbid.arg("-c").arg(&forbid_cfg).arg(&merge_dup);
        let (ryl_merge_forbid_code, ryl_merge_forbid_msg) =
            capture_with_env(ryl_merge_forbid, scenario.envs);

        let mut yam_merge_forbid = build_yamllint_command(scenario.yam_format);
        yam_merge_forbid.arg("-c").arg(&forbid_cfg).arg(&merge_dup);
        let (yam_merge_forbid_code, yam_merge_forbid_msg) =
            capture_with_env(yam_merge_forbid, scenario.envs);

        assert_eq!(
            ryl_merge_forbid_code, 1,
            "ryl merge forbid exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_merge_forbid_code, 1,
            "yamllint merge forbid exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_merge_forbid_msg, yam_merge_forbid_msg,
            "merge forbid diagnostics mismatch ({})",
            scenario.label
        );
    }
}
