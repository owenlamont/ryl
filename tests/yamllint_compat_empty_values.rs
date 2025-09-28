use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn empty_values_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let violations = dir.path().join("violations.yaml");
    fs::write(
        &violations,
        "block:\n  missing:\nflow: { missing: }\nseq:\n  -\n",
    )
    .unwrap();

    let clean = dir.path().join("clean.yaml");
    fs::write(
        &clean,
        "block:\n  present: value\nflow: { present: value }\nseq:\n  - value\n",
    )
    .unwrap();

    let all_cfg = dir.path().join("empty-all.yml");
    fs::write(
        &all_cfg,
        "rules:\n  document-start: disable\n  empty-values: enable\n",
    )
    .unwrap();

    let block_cfg = dir.path().join("empty-block.yml");
    fs::write(
        &block_cfg,
        "rules:\n  document-start: disable\n  empty-values:\n    forbid-in-block-mappings: true\n    forbid-in-flow-mappings: false\n    forbid-in-block-sequences: false\n",
    )
    .unwrap();

    let flow_cfg = dir.path().join("empty-flow.yml");
    fs::write(
        &flow_cfg,
        "rules:\n  document-start: disable\n  empty-values:\n    forbid-in-block-mappings: false\n    forbid-in-flow-mappings: true\n    forbid-in-block-sequences: false\n",
    )
    .unwrap();

    let seq_cfg = dir.path().join("empty-seq.yml");
    fs::write(
        &seq_cfg,
        "rules:\n  document-start: disable\n  empty-values:\n    forbid-in-block-mappings: false\n    forbid-in-flow-mappings: false\n    forbid-in-block-sequences: true\n",
    )
    .unwrap();

    let none_cfg = dir.path().join("empty-none.yml");
    fs::write(
        &none_cfg,
        "rules:\n  document-start: disable\n  empty-values:\n    forbid-in-block-mappings: false\n    forbid-in-flow-mappings: false\n    forbid-in-block-sequences: false\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
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

        let mut ryl_block = build_ryl_command(exe, scenario.ryl_format);
        ryl_block.arg("-c").arg(&block_cfg).arg(&violations);
        let (ryl_block_code, ryl_block_msg) = capture_with_env(ryl_block, scenario.envs);

        let mut yam_block = build_yamllint_command(scenario.yam_format);
        yam_block.arg("-c").arg(&block_cfg).arg(&violations);
        let (yam_block_code, yam_block_msg) = capture_with_env(yam_block, scenario.envs);

        assert_eq!(ryl_block_code, 1, "ryl block exit ({})", scenario.label);
        assert_eq!(
            yam_block_code, 1,
            "yamllint block exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_block_msg, yam_block_msg,
            "block diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_flow = build_ryl_command(exe, scenario.ryl_format);
        ryl_flow.arg("-c").arg(&flow_cfg).arg(&violations);
        let (ryl_flow_code, ryl_flow_msg) = capture_with_env(ryl_flow, scenario.envs);

        let mut yam_flow = build_yamllint_command(scenario.yam_format);
        yam_flow.arg("-c").arg(&flow_cfg).arg(&violations);
        let (yam_flow_code, yam_flow_msg) = capture_with_env(yam_flow, scenario.envs);

        assert_eq!(ryl_flow_code, 1, "ryl flow exit ({})", scenario.label);
        assert_eq!(yam_flow_code, 1, "yamllint flow exit ({})", scenario.label);
        assert_eq!(
            ryl_flow_msg, yam_flow_msg,
            "flow diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_seq = build_ryl_command(exe, scenario.ryl_format);
        ryl_seq.arg("-c").arg(&seq_cfg).arg(&violations);
        let (ryl_seq_code, ryl_seq_msg) = capture_with_env(ryl_seq, scenario.envs);

        let mut yam_seq = build_yamllint_command(scenario.yam_format);
        yam_seq.arg("-c").arg(&seq_cfg).arg(&violations);
        let (yam_seq_code, yam_seq_msg) = capture_with_env(yam_seq, scenario.envs);

        assert_eq!(ryl_seq_code, 1, "ryl seq exit ({})", scenario.label);
        assert_eq!(yam_seq_code, 1, "yamllint seq exit ({})", scenario.label);
        assert_eq!(
            ryl_seq_msg, yam_seq_msg,
            "seq diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_none = build_ryl_command(exe, scenario.ryl_format);
        ryl_none.arg("-c").arg(&none_cfg).arg(&violations);
        let (ryl_none_code, ryl_none_msg) = capture_with_env(ryl_none, scenario.envs);

        let mut yam_none = build_yamllint_command(scenario.yam_format);
        yam_none.arg("-c").arg(&none_cfg).arg(&violations);
        let (yam_none_code, yam_none_msg) = capture_with_env(yam_none, scenario.envs);

        assert_eq!(ryl_none_code, 0, "ryl none exit ({})", scenario.label);
        assert_eq!(yam_none_code, 0, "yamllint none exit ({})", scenario.label);
        assert_eq!(
            ryl_none_msg, yam_none_msg,
            "none diagnostics mismatch ({})",
            scenario.label
        );

        let mut ryl_clean = build_ryl_command(exe, scenario.ryl_format);
        ryl_clean.arg("-c").arg(&all_cfg).arg(&clean);
        let (ryl_clean_code, ryl_clean_msg) = capture_with_env(ryl_clean, scenario.envs);

        let mut yam_clean = build_yamllint_command(scenario.yam_format);
        yam_clean.arg("-c").arg(&all_cfg).arg(&clean);
        let (yam_clean_code, yam_clean_msg) = capture_with_env(yam_clean, scenario.envs);

        assert_eq!(ryl_clean_code, 0, "ryl clean exit ({})", scenario.label);
        assert_eq!(
            yam_clean_code, 0,
            "yamllint clean exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_clean_msg, yam_clean_msg,
            "clean diagnostics mismatch ({})",
            scenario.label
        );
    }
}
