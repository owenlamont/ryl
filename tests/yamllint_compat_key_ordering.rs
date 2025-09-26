use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[path = "common/compat.rs"]
mod compat;

use compat::{
    SCENARIOS, STANDARD_ENV, build_ryl_command, build_yamllint_command, capture_with_env,
    ensure_yamllint_installed,
};

#[test]
fn key_ordering_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("key-ordering-default.yml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  key-ordering: enable\n",
    )
    .unwrap();

    let ignored_cfg = dir.path().join("key-ordering-ignored.yml");
    fs::write(
        &ignored_cfg,
        "rules:\n  document-start: disable\n  key-ordering:\n    ignored-keys: [\"n(a|o)me\", \"^b\"]\n",
    )
    .unwrap();

    let locale_cfg = dir.path().join("key-ordering-locale.yml");
    fs::write(
        &locale_cfg,
        "locale: en_US.UTF-8\nrules:\n  document-start: disable\n  key-ordering: enable\n",
    )
    .unwrap();

    let bad_block = dir.path().join("bad-block.yaml");
    fs::write(
        &bad_block,
        "block mapping:\n  second: value\n  first: value\n",
    )
    .unwrap();

    let good_block = dir.path().join("good-block.yaml");
    fs::write(
        &good_block,
        "block mapping:\n  first: value\n  second: value\n  third: v\n",
    )
    .unwrap();

    let ignored_file = dir.path().join("ignored.yaml");
    fs::write(
        &ignored_file,
        "a:\nb:\nc:\nname: ignored\nfirst-name: ignored\nnome: ignored\ngnomes: ignored\nd:\ne:\nboat: ignored\n.boat: ERROR\ncall: ERROR\nf:\ng:\n",
    )
    .unwrap();

    let locale_file = dir.path().join("locale.yaml");
    fs::write(
        &locale_file,
        "- t-shirt: 1\n  T-shirt: 2\n  t-shirts: 3\n  T-shirts: 4\n- hair: true\n  haïr: true\n  hais: true\n  haïssable: true\n",
    )
    .unwrap();

    // Probe locale availability before running the full matrix. Skip locale comparison when
    // yamllint cannot set the configured locale.
    let mut locale_probe = Command::new("yamllint");
    locale_probe.arg("-c").arg(&locale_cfg).arg(&locale_file);
    let (probe_code, probe_output) = capture_with_env(locale_probe, STANDARD_ENV);
    let skip_locale = if probe_code != 0
        && (probe_output.contains("unsupported locale") || probe_output.contains("unknown locale"))
    {
        eprintln!("skipping locale comparison: {probe_output}");
        true
    } else {
        false
    };

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        // default configuration, expect violations on bad block
        let mut ryl_bad = build_ryl_command(exe, scenario.ryl_format);
        ryl_bad.arg("-c").arg(&default_cfg).arg(&bad_block);
        let (ryl_bad_code, ryl_bad_msg) = capture_with_env(ryl_bad, scenario.envs);

        let mut yam_bad = build_yamllint_command(scenario.yam_format);
        yam_bad.arg("-c").arg(&default_cfg).arg(&bad_block);
        let (yam_bad_code, yam_bad_msg) = capture_with_env(yam_bad, scenario.envs);

        assert_eq!(ryl_bad_code, 1, "ryl bad exit ({})", scenario.label);
        assert_eq!(yam_bad_code, 1, "yamllint bad exit ({})", scenario.label);
        assert_eq!(
            ryl_bad_msg, yam_bad_msg,
            "bad block diagnostics mismatch ({})",
            scenario.label
        );

        // good block should be clean
        let mut ryl_good = build_ryl_command(exe, scenario.ryl_format);
        ryl_good.arg("-c").arg(&default_cfg).arg(&good_block);
        let (ryl_good_code, ryl_good_msg) = capture_with_env(ryl_good, scenario.envs);

        let mut yam_good = build_yamllint_command(scenario.yam_format);
        yam_good.arg("-c").arg(&default_cfg).arg(&good_block);
        let (yam_good_code, yam_good_msg) = capture_with_env(yam_good, scenario.envs);

        assert_eq!(ryl_good_code, 0, "ryl good exit ({})", scenario.label);
        assert_eq!(yam_good_code, 0, "yamllint good exit ({})", scenario.label);
        assert_eq!(
            ryl_good_msg, yam_good_msg,
            "good block diagnostics mismatch ({})",
            scenario.label
        );

        // ignored-keys configuration should allow ignored patterns and flag others equally
        let mut ryl_ignored = build_ryl_command(exe, scenario.ryl_format);
        ryl_ignored.arg("-c").arg(&ignored_cfg).arg(&ignored_file);
        let (ryl_ignored_code, ryl_ignored_msg) = capture_with_env(ryl_ignored, scenario.envs);

        let mut yam_ignored = build_yamllint_command(scenario.yam_format);
        yam_ignored.arg("-c").arg(&ignored_cfg).arg(&ignored_file);
        let (yam_ignored_code, yam_ignored_msg) = capture_with_env(yam_ignored, scenario.envs);

        assert_eq!(ryl_ignored_code, 1, "ryl ignored exit ({})", scenario.label);
        assert_eq!(
            yam_ignored_code, 1,
            "yamllint ignored exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_ignored_msg, yam_ignored_msg,
            "ignored diagnostics mismatch ({})",
            scenario.label
        );

        if skip_locale {
            continue;
        }

        let mut ryl_locale = build_ryl_command(exe, scenario.ryl_format);
        ryl_locale.arg("-c").arg(&locale_cfg).arg(&locale_file);
        let (ryl_locale_code, ryl_locale_msg) = capture_with_env(ryl_locale, scenario.envs);

        let mut yam_locale = build_yamllint_command(scenario.yam_format);
        yam_locale.arg("-c").arg(&locale_cfg).arg(&locale_file);
        let (yam_locale_code, yam_locale_msg) = capture_with_env(yam_locale, scenario.envs);

        assert_eq!(ryl_locale_code, 0, "ryl locale exit ({})", scenario.label);
        assert_eq!(
            yam_locale_code, 0,
            "yamllint locale exit ({})",
            scenario.label
        );
        assert_eq!(
            ryl_locale_msg, yam_locale_msg,
            "locale diagnostics mismatch ({})",
            scenario.label
        );
    }
}
