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
fn quoted_strings_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let default_cfg = dir.path().join("default.yaml");
    fs::write(
        &default_cfg,
        "rules:\n  document-start: disable\n  quoted-strings: enable\n",
    )
    .unwrap();

    let only_needed_cfg = dir.path().join("only-needed.yaml");
    fs::write(
        &only_needed_cfg,
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    )
    .unwrap();

    let extra_required_cfg = dir.path().join("extra-required.yaml");
    fs::write(
        &extra_required_cfg,
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: false\n    extra-required: ['^http']\n",
    )
    .unwrap();

    let extra_allowed_cfg = dir.path().join("extra-allowed.yaml");
    fs::write(
        &extra_allowed_cfg,
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n    extra-allowed: ['^http']\n",
    )
    .unwrap();

    let double_allow_cfg = dir.path().join("double-allow.yaml");
    fs::write(
        &double_allow_cfg,
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n    allow-quoted-quotes: true\n",
    )
    .unwrap();

    let check_keys_cfg = dir.path().join("check-keys.yaml");
    fs::write(
        &check_keys_cfg,
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n    check-keys: true\n    extra-required: ['[:]']\n",
    )
    .unwrap();

    let plain_file = dir.path().join("plain.yaml");
    fs::write(&plain_file, "foo: bar\n").unwrap();

    let quoted_file = dir.path().join("quoted.yaml");
    fs::write(&quoted_file, "foo: \"bar\"\n").unwrap();

    let url_file = dir.path().join("urls.yaml");
    fs::write(&url_file, "- http://example.com\n").unwrap();

    let allowed_url_file = dir.path().join("allowed-url.yaml");
    fs::write(&allowed_url_file, "foo: \"http://example.com\"\n").unwrap();

    let quoted_quotes_file = dir.path().join("quoted-quotes.yaml");
    fs::write(&quoted_quotes_file, "foo: 'bar\"baz'\n").unwrap();

    let key_file = dir.path().join("key.yaml");
    fs::write(&key_file, "foo:bar: baz\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    let cases = [
        Case {
            label: "default-plain",
            config: &default_cfg,
            file: &plain_file,
            exit: 1,
        },
        Case {
            label: "only-needed-quoted",
            config: &only_needed_cfg,
            file: &quoted_file,
            exit: 1,
        },
        Case {
            label: "extra-required-url",
            config: &extra_required_cfg,
            file: &url_file,
            exit: 1,
        },
        Case {
            label: "extra-allowed-url",
            config: &extra_allowed_cfg,
            file: &allowed_url_file,
            exit: 0,
        },
        Case {
            label: "double-allow-quotes",
            config: &double_allow_cfg,
            file: &quoted_quotes_file,
            exit: 0,
        },
        Case {
            label: "check-keys",
            config: &check_keys_cfg,
            file: &key_file,
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
