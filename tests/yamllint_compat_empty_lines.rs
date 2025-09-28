use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

struct Case {
    label: &'static str,
    config: std::path::PathBuf,
    target: std::path::PathBuf,
    expected_exit: i32,
}

#[test]
fn empty_lines_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let interior_file = dir.path().join("interior.yaml");
    fs::write(&interior_file, "key: 1\n\n\nvalue: 2\n").unwrap();
    let start_file = dir.path().join("start.yaml");
    fs::write(&start_file, "\n\nkey: 1\n").unwrap();
    let end_file = dir.path().join("end.yaml");
    fs::write(&end_file, "key: 1\n\n\n").unwrap();

    let interior_cfg = dir.path().join("interior.yml");
    fs::write(
        &interior_cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: disable\n  empty-lines:\n    max: 1\n    max-start: 0\n    max-end: 0\n",
    )
    .unwrap();

    let warning_cfg = dir.path().join("warning.yml");
    fs::write(
        &warning_cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: disable\n  empty-lines:\n    level: warning\n    max: 1\n    max-start: 0\n    max-end: 0\n",
    )
    .unwrap();

    let start_cfg = dir.path().join("start.yml");
    fs::write(
        &start_cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: disable\n  empty-lines:\n    max: 5\n    max-start: 1\n    max-end: 5\n",
    )
    .unwrap();

    let end_cfg = dir.path().join("end.yml");
    fs::write(
        &end_cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: disable\n  empty-lines:\n    max: 5\n    max-start: 5\n    max-end: 1\n",
    )
    .unwrap();

    let cases = vec![
        Case {
            label: "interior-error",
            config: interior_cfg.clone(),
            target: interior_file.clone(),
            expected_exit: 1,
        },
        Case {
            label: "interior-warning",
            config: warning_cfg.clone(),
            target: interior_file,
            expected_exit: 0,
        },
        Case {
            label: "start-error",
            config: start_cfg.clone(),
            target: start_file,
            expected_exit: 1,
        },
        Case {
            label: "end-error",
            config: end_cfg.clone(),
            target: end_file,
            expected_exit: 1,
        },
    ];

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        for case in &cases {
            let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
            ryl_cmd.arg("-c").arg(&case.config).arg(&case.target);
            let (ryl_code, ryl_output) = capture_with_env(ryl_cmd, scenario.envs);

            let mut yam_cmd = build_yamllint_command(scenario.yam_format);
            yam_cmd.arg("-c").arg(&case.config).arg(&case.target);
            let (yam_code, yam_output) = capture_with_env(yam_cmd, scenario.envs);

            assert_eq!(
                ryl_code, case.expected_exit,
                "ryl exit mismatch ({}/{})",
                scenario.label, case.label
            );
            assert_eq!(
                yam_code, case.expected_exit,
                "yamllint exit mismatch ({}/{})",
                scenario.label, case.label
            );
            assert_eq!(
                ryl_output, yam_output,
                "diagnostics mismatch ({}/{})",
                scenario.label, case.label
            );
        }
    }
}
