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
    config_path: Option<std::path::PathBuf>,
    expected_exit: i32,
}

#[test]
fn colored_diagnostics_match_yamllint_across_formats() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let file = dir.path().join("layout.yaml");
    fs::write(&file, "list: [1,2]\n").unwrap();

    let warning_cfg = dir.path().join("layout-warning.yml");
    fs::write(&warning_cfg, "rules:\n  commas:\n    level: warning\n").unwrap();

    let cases = [
        Case {
            label: "default-config",
            config_path: None,
            expected_exit: 1,
        },
        Case {
            label: "commas-warning",
            config_path: Some(warning_cfg.clone()),
            expected_exit: 0,
        },
    ];

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        for case in &cases {
            let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
            if let Some(cfg) = &case.config_path {
                ryl_cmd.arg("-c").arg(cfg);
            }
            ryl_cmd.arg(&file);
            let (ryl_code, ryl_output) = capture_with_env(ryl_cmd, scenario.envs);

            let mut yam_cmd = build_yamllint_command(scenario.yam_format);
            if let Some(cfg) = &case.config_path {
                yam_cmd.arg("-c").arg(cfg);
            }
            yam_cmd.arg(&file);
            let (yam_code, yam_output) = capture_with_env(yam_cmd, scenario.envs);

            assert_eq!(
                ryl_code, case.expected_exit,
                "ryl exit code mismatch (scenario: {}, case: {})",
                scenario.label, case.label
            );
            assert_eq!(
                yam_code, case.expected_exit,
                "yamllint exit code mismatch (scenario: {}, case: {})",
                scenario.label, case.label
            );
            assert_eq!(
                ryl_output, yam_output,
                "diagnostic mismatch (scenario: {}, case: {})",
                scenario.label, case.label
            );
        }
    }
}
