use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn yaml_files_patterns_match_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let default_cfg = dir.path().join("yaml-files-default.yml");
    let negated_cfg = dir.path().join("yaml-files-negated.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  truthy: enable\nyaml-files: ['*.yaml']\n",
    )
    .unwrap();
    fs::write(
        &negated_cfg,
        "rules:\n  document-start: disable\n  truthy: enable\nyaml-files: ['*.yaml', '!skip.yaml']\n",
    )
    .unwrap();

    let keep = dir.path().join("keep.yaml");
    let skip = dir.path().join("skip.yaml");
    let other = dir.path().join("note.yml");
    fs::write(&keep, "value: Yes\n").unwrap();
    fs::write(&skip, "value: Yes\n").unwrap();
    fs::write(&other, "value: Yes\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_default.arg("-c").arg(&default_cfg).arg(dir.path());
        let (ryl_default_code, ryl_default_out) = capture_with_env(ryl_default, scenario.envs);

        let mut yam_default = build_yamllint_command(scenario.yam_format);
        yam_default.arg("-c").arg(&default_cfg).arg(dir.path());
        let (yam_default_code, yam_default_out) = capture_with_env(yam_default, scenario.envs);

        assert_eq!(
            ryl_default_code, yam_default_code,
            "default yaml-files exit mismatch ({})",
            scenario.label
        );
        assert_eq!(
            ryl_default_out, yam_default_out,
            "default yaml-files diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_negated = build_ryl_command(exe, scenario.ryl_format);
        ryl_negated.arg("-c").arg(&negated_cfg).arg(dir.path());
        let (ryl_neg_code, ryl_neg_out) = capture_with_env(ryl_negated, scenario.envs);

        let mut yam_negated = build_yamllint_command(scenario.yam_format);
        yam_negated.arg("-c").arg(&negated_cfg).arg(dir.path());
        let (yam_neg_code, yam_neg_out) = capture_with_env(yam_negated, scenario.envs);

        assert_eq!(
            ryl_neg_code, yam_neg_code,
            "negated yaml-files exit mismatch ({})",
            scenario.label
        );
        assert_eq!(
            ryl_neg_out, yam_neg_out,
            "negated yaml-files diagnostics mismatch ({})",
            scenario.label
        );
    }
}
