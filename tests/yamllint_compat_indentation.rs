use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

struct Case<'a> {
    label: &'a str,
    config: &'a std::path::Path,
    file: &'a std::path::Path,
    exit: i32,
}

#[test]
fn indentation_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let base_cfg = dir.path().join("base.yaml");
    fs::write(
        &base_cfg,
        "rules:\n  document-start: disable\n  indentation:\n    spaces: 2\n",
    )
    .unwrap();

    let seq_cfg = dir.path().join("seq.yaml");
    fs::write(
        &seq_cfg,
        "rules:\n  document-start: disable\n  indentation:\n    spaces: 2\n    indent-sequences: true\n",
    )
    .unwrap();

    let seq_off_cfg = dir.path().join("seq-off.yaml");
    fs::write(
        &seq_off_cfg,
        "rules:\n  document-start: disable\n  indentation:\n    indent-sequences: false\n",
    )
    .unwrap();

    let multi_cfg = dir.path().join("multi.yaml");
    fs::write(
        &multi_cfg,
        "rules:\n  document-start: disable\n  indentation:\n    spaces: 4\n    check-multi-line-strings: true\n",
    )
    .unwrap();

    let map_file = dir.path().join("map.yaml");
    fs::write(&map_file, "root:\n   child: value\n").unwrap();

    let seq_bad_file = dir.path().join("seq-bad.yaml");
    fs::write(&seq_bad_file, "root:\n- item\n").unwrap();

    let seq_ok_file = dir.path().join("seq-ok.yaml");
    fs::write(&seq_ok_file, "root:\n  - item\n").unwrap();

    let seq_over_file = dir.path().join("seq-over.yaml");
    fs::write(&seq_over_file, "root:\n      - item\n").unwrap();

    let multi_bad_file = dir.path().join("multi-bad.yaml");
    fs::write(&multi_bad_file, "quote: |\n    good\n     bad\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    let cases = [
        Case {
            label: "mapping-indent",
            config: &base_cfg,
            file: &map_file,
            exit: 1,
        },
        Case {
            label: "sequence-indent-required",
            config: &seq_cfg,
            file: &seq_bad_file,
            exit: 1,
        },
        Case {
            label: "sequence-indent-disabled",
            config: &seq_off_cfg,
            file: &seq_bad_file,
            exit: 0,
        },
        Case {
            label: "sequence-ok",
            config: &seq_cfg,
            file: &seq_ok_file,
            exit: 0,
        },
        Case {
            label: "sequence-over-indented",
            config: &seq_cfg,
            file: &seq_over_file,
            exit: 1,
        },
        Case {
            label: "multi-line",
            config: &multi_cfg,
            file: &multi_bad_file,
            exit: 1,
        },
    ];

    for scenario in SCENARIOS {
        for case in &cases {
            let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
            ryl_cmd.arg("-c").arg(case.config).arg(case.file);
            let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

            let mut yam_cmd = build_yamllint_command(scenario.yam_format);
            yam_cmd.arg("-c").arg(case.config).arg(case.file);
            let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

            assert_eq!(
                ryl_code, case.exit,
                "ryl exit mismatch {} ({})",
                case.label, scenario.label
            );
            assert_eq!(
                yam_code, case.exit,
                "yamllint exit mismatch {} ({})",
                case.label, scenario.label
            );
            assert_eq!(
                ryl_msg, yam_msg,
                "diagnostics mismatch {} ({})",
                case.label, scenario.label
            );
        }
    }
}
