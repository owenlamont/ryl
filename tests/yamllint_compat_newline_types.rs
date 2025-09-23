use std::fs;
use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn new_lines_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let unix_cfg = dir.path().join("config-unix.yml");
    fs::write(
        &unix_cfg,
        "rules:\n  document-start: disable\n  new-lines:\n    type: unix\n",
    )
    .unwrap();
    let unix_warning_cfg = dir.path().join("config-unix-warning.yml");
    fs::write(
        &unix_warning_cfg,
        "rules:\n  document-start: disable\n  new-lines:\n    level: warning\n",
    )
    .unwrap();

    let dos_cfg = dir.path().join("config-dos.yml");
    fs::write(
        &dos_cfg,
        "rules:\n  document-start: disable\n  new-lines:\n    type: dos\n",
    )
    .unwrap();

    let platform_cfg = dir.path().join("config-platform.yml");
    fs::write(
        &platform_cfg,
        "rules:\n  document-start: disable\n  new-lines:\n    type: platform\n",
    )
    .unwrap();

    let lf_file = dir.path().join("lf.yaml");
    fs::write(&lf_file, "key: value\n").unwrap();
    let crlf_file = dir.path().join("crlf.yaml");
    fs::write(&crlf_file, "key: value\r\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        // unix type mismatch
        let mut ryl_unix = build_ryl_command(exe, scenario.ryl_format);
        ryl_unix.arg("-c").arg(&unix_cfg).arg(&crlf_file);
        let (ryl_unix_code, ryl_unix_msg) = capture_with_env(ryl_unix, scenario.envs);

        let mut yam_unix = build_yamllint_command(scenario.yam_format);
        yam_unix.arg("-c").arg(&unix_cfg).arg(&crlf_file);
        let (yam_unix_code, yam_unix_msg) = capture_with_env(yam_unix, scenario.envs);

        assert_eq!(ryl_unix_code, 1, "ryl unix mismatch ({})", scenario.label);
        assert_eq!(
            yam_unix_code, 1,
            "yamllint unix mismatch ({})",
            scenario.label
        );
        assert_eq!(
            ryl_unix_msg, yam_unix_msg,
            "unix mismatch diagnostics ({})",
            scenario.label
        );

        // dos type mismatch
        let mut ryl_dos = build_ryl_command(exe, scenario.ryl_format);
        ryl_dos.arg("-c").arg(&dos_cfg).arg(&lf_file);
        let (ryl_dos_code, ryl_dos_msg) = capture_with_env(ryl_dos, scenario.envs);

        let mut yam_dos = build_yamllint_command(scenario.yam_format);
        yam_dos.arg("-c").arg(&dos_cfg).arg(&lf_file);
        let (yam_dos_code, yam_dos_msg) = capture_with_env(yam_dos, scenario.envs);

        assert_eq!(ryl_dos_code, 1, "ryl dos mismatch ({})", scenario.label);
        assert_eq!(
            yam_dos_code, 1,
            "yamllint dos mismatch ({})",
            scenario.label
        );
        assert_eq!(
            ryl_dos_msg, yam_dos_msg,
            "dos mismatch diagnostics ({})",
            scenario.label
        );

        // platform mismatch depends on runtime platform
        let (platform_file, platform_label) = if cfg!(windows) {
            (&lf_file, "\\r\\n")
        } else {
            (&crlf_file, "\\n")
        };

        let mut ryl_platform = build_ryl_command(exe, scenario.ryl_format);
        ryl_platform.arg("-c").arg(&platform_cfg).arg(platform_file);
        let (ryl_platform_code, ryl_platform_msg) = capture_with_env(ryl_platform, scenario.envs);

        let mut yam_platform = build_yamllint_command(scenario.yam_format);
        yam_platform.arg("-c").arg(&platform_cfg).arg(platform_file);
        let (yam_platform_code, yam_platform_msg) = capture_with_env(yam_platform, scenario.envs);

        assert_eq!(
            ryl_platform_code, 1,
            "ryl platform mismatch ({})",
            scenario.label
        );
        assert_eq!(
            yam_platform_code, 1,
            "yamllint platform mismatch ({})",
            scenario.label
        );
        assert_eq!(
            ryl_platform_msg, yam_platform_msg,
            "platform mismatch diagnostics ({})",
            scenario.label
        );
        assert!(
            ryl_platform_msg.contains(platform_label),
            "platform message should mention expected {platform_label} ({})",
            scenario.label
        );

        // success path for dos config with CRLF
        let mut ryl_dos_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_dos_ok.arg("-c").arg(&dos_cfg).arg(&crlf_file);
        let (ryl_dos_ok_code, ryl_dos_ok_msg) = capture_with_env(ryl_dos_ok, scenario.envs);

        let mut yam_dos_ok = build_yamllint_command(scenario.yam_format);
        yam_dos_ok.arg("-c").arg(&dos_cfg).arg(&crlf_file);
        let (yam_dos_ok_code, yam_dos_ok_msg) = capture_with_env(yam_dos_ok, scenario.envs);

        assert_eq!(
            ryl_dos_ok_code, 0,
            "dos success code mismatch ({})",
            scenario.label
        );
        assert_eq!(
            yam_dos_ok_code, 0,
            "yamllint dos success code ({})",
            scenario.label
        );
        assert_eq!(
            ryl_dos_ok_msg, yam_dos_ok_msg,
            "expected identical diagnostics for dos success ({})",
            scenario.label
        );

        // warning level behavior
        let mut ryl_warn = build_ryl_command(exe, scenario.ryl_format);
        ryl_warn.arg("-c").arg(&unix_warning_cfg).arg(&crlf_file);
        let (ryl_warn_code, ryl_warn_msg) = capture_with_env(ryl_warn, scenario.envs);

        let mut yam_warn = build_yamllint_command(scenario.yam_format);
        yam_warn.arg("-c").arg(&unix_warning_cfg).arg(&crlf_file);
        let (yam_warn_code, yam_warn_msg) = capture_with_env(yam_warn, scenario.envs);

        assert_eq!(ryl_warn_code, 0, "ryl warning exit ({})", scenario.label);
        assert_eq!(
            yam_warn_code, 0,
            "yamllint warning exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_warn_msg, yam_warn_msg,
            "warning diagnostics should match ({})",
            scenario.label
        );
        assert!(
            ryl_warn_msg.contains("warning"),
            "warning output should mention warning ({})",
            scenario.label
        );
    }
}
