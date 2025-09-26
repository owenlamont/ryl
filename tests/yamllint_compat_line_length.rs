use std::fs;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn line_length_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("line-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  empty-lines: disable\n  new-line-at-end-of-file: disable\n  line-length:\n    max: 80\n",
    )
    .unwrap();

    let warning_cfg = dir.path().join("line-warning.yml");
    fs::write(
        &warning_cfg,
        "rules:\n  document-start: disable\n  empty-lines: disable\n  new-line-at-end-of-file: disable\n  line-length:\n    max: 80\n    level: warning\n",
    )
    .unwrap();

    let strict_cfg = dir.path().join("line-strict.yml");
    fs::write(
        &strict_cfg,
        "rules:\n  document-start: disable\n  line-length:\n    max: 20\n    allow-non-breakable-words: false\n",
    )
    .unwrap();

    let inline_cfg = dir.path().join("line-inline.yml");
    fs::write(
        &inline_cfg,
        "rules:\n  document-start: disable\n  line-length:\n    max: 20\n    allow-non-breakable-inline-mappings: true\n",
    )
    .unwrap();

    let bad_file = dir.path().join("bad.yaml");
    fs::write(
        &bad_file,
        "key: this line is definitely longer than eighty characters so yamllint and ryl should both flag it\n",
    )
    .unwrap();

    let long_word_file = dir.path().join("long_word.yaml");
    fs::write(&long_word_file, format!("{}\n", "A".repeat(50))).unwrap();

    let inline_ok_file = dir.path().join("inline_ok.yaml");
    fs::write(
        &inline_ok_file,
        "url: http://example.com/very/very/very/very/very/very/long/path\n",
    )
    .unwrap();

    let inline_bad_file = dir.path().join("inline_bad.yaml");
    fs::write(
        &inline_bad_file,
        "url: http://example.com/short + extra words to trigger the rule\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        // default configuration should produce an error for spaced text
        let mut ryl_default = build_ryl_command(exe, scenario.ryl_format);
        ryl_default.arg("-c").arg(&default_cfg).arg(&bad_file);
        let (ryl_code, ryl_output) = capture_with_env(ryl_default, scenario.envs);

        let mut yam_default = build_yamllint_command(scenario.yam_format);
        yam_default.arg("-c").arg(&default_cfg).arg(&bad_file);
        let (yam_code, yam_output) = capture_with_env(yam_default, scenario.envs);

        assert_eq!(ryl_code, 1, "ryl default exit ({})", scenario.label);
        assert_eq!(yam_code, 1, "yamllint default exit ({})", scenario.label);
        assert_eq!(
            ryl_output, yam_output,
            "default diagnostics mismatch ({})",
            scenario.label
        );

        // warning configuration should keep diagnostics but exit 0
        let mut ryl_warning = build_ryl_command(exe, scenario.ryl_format);
        ryl_warning.arg("-c").arg(&warning_cfg).arg(&bad_file);
        let (ryl_warn_code, ryl_warn_output) = capture_with_env(ryl_warning, scenario.envs);

        let mut yam_warning = build_yamllint_command(scenario.yam_format);
        yam_warning.arg("-c").arg(&warning_cfg).arg(&bad_file);
        let (yam_warn_code, yam_warn_output) = capture_with_env(yam_warning, scenario.envs);

        assert_eq!(ryl_warn_code, 0, "ryl warning exit ({})", scenario.label);
        assert_eq!(
            yam_warn_code, 0,
            "yamllint warning exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_warn_output, yam_warn_output,
            "warning diagnostics mismatch ({})",
            scenario.label
        );

        // strict configuration should flag long non-breakable words
        let mut ryl_strict = build_ryl_command(exe, scenario.ryl_format);
        ryl_strict.arg("-c").arg(&strict_cfg).arg(&long_word_file);
        let (ryl_strict_code, ryl_strict_output) = capture_with_env(ryl_strict, scenario.envs);

        let mut yam_strict = build_yamllint_command(scenario.yam_format);
        yam_strict.arg("-c").arg(&strict_cfg).arg(&long_word_file);
        let (yam_strict_code, yam_strict_output) = capture_with_env(yam_strict, scenario.envs);

        assert_eq!(ryl_strict_code, 1, "ryl strict exit ({})", scenario.label);
        assert_eq!(
            yam_strict_code, 1,
            "yamllint strict exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_strict_output, yam_strict_output,
            "strict diagnostics mismatch ({})",
            scenario.label
        );

        // inline mapping allowance should let long URLs through
        let mut ryl_inline_ok = build_ryl_command(exe, scenario.ryl_format);
        ryl_inline_ok
            .arg("-c")
            .arg(&inline_cfg)
            .arg(&inline_ok_file);
        let (ryl_inline_code, ryl_inline_output) = capture_with_env(ryl_inline_ok, scenario.envs);

        let mut yam_inline_ok = build_yamllint_command(scenario.yam_format);
        yam_inline_ok
            .arg("-c")
            .arg(&inline_cfg)
            .arg(&inline_ok_file);
        let (yam_inline_code, yam_inline_output) = capture_with_env(yam_inline_ok, scenario.envs);

        assert_eq!(
            ryl_inline_code, 0,
            "ryl inline-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_inline_code, 0,
            "yamllint inline-ok exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_inline_output, yam_inline_output,
            "inline ok diagnostics mismatch ({})",
            scenario.label
        );

        // inline mapping option should still flag when spaces remain
        let mut ryl_inline_bad = build_ryl_command(exe, scenario.ryl_format);
        ryl_inline_bad
            .arg("-c")
            .arg(&inline_cfg)
            .arg(&inline_bad_file);
        let (ryl_inline_bad_code, ryl_inline_bad_output) =
            capture_with_env(ryl_inline_bad, scenario.envs);

        let mut yam_inline_bad = build_yamllint_command(scenario.yam_format);
        yam_inline_bad
            .arg("-c")
            .arg(&inline_cfg)
            .arg(&inline_bad_file);
        let (yam_inline_bad_code, yam_inline_bad_output) =
            capture_with_env(yam_inline_bad, scenario.envs);

        assert_eq!(
            ryl_inline_bad_code, 1,
            "ryl inline-bad exit ({})",
            scenario.label
        );
        assert_eq!(
            yam_inline_bad_code, 1,
            "yamllint inline-bad exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_inline_bad_output, yam_inline_bad_output,
            "inline bad diagnostics mismatch ({})",
            scenario.label
        );
    }
}
