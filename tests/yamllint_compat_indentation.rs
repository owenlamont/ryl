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

#[test]
fn nested_sequence_indentation_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let cfg_path = dir.path().join("cfg.yaml");
    fs::write(
        &cfg_path,
        "rules:\n  document-start: disable\n  indentation:\n    spaces: 2\n    indent-sequences: consistent\n",
    )
    .unwrap();

    let yaml_path = dir.path().join("input.yaml");
    fs::write(
        &yaml_path,
        "root:\n  - any:\n      - changed-files:\n          - any-glob-to-any-file:\n              - foo\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(
            ryl_code, 0,
            "ryl exited with diagnostics ({})",
            scenario.label
        );
        assert_eq!(
            yam_code, 0,
            "yamllint reported unexpected diagnostics ({})",
            scenario.label
        );
        assert_eq!(ryl_msg, yam_msg, "output mismatch ({})", scenario.label);
    }
}

#[test]
fn sequence_after_comments_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let cfg_path = dir.path().join("cfg.yaml");
    fs::write(
        &cfg_path,
        "rules:\n  document-start: disable\n  line-length: disable\n  indentation:\n    spaces: 2\n",
    )
    .unwrap();

    let yaml_path = dir.path().join("input.yaml");
    fs::write(
        &yaml_path,
        "bug:\n  # Bug Issue Template:\n  #   [Bug]:\n  # General:\n  #   panic:\n  # Terraform Plugin SDK:\n  #   doesn't support update\n  #   Invalid address to set\n  - \"(\\\\[Bug\\\\]:|doesn't support update|Invalid address to set|panic:)\"\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(
            yam_code, 0,
            "yamllint should accept input ({})",
            scenario.label
        );
        assert_eq!(
            ryl_code, 0,
            "ryl should accept input ({})\n{}",
            scenario.label, ryl_msg
        );
        assert_eq!(ryl_msg, yam_msg, "output mismatch ({})", scenario.label);
    }
}

#[test]
fn compact_nested_sequence_indentation_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let cfg_path = dir.path().join("cfg.yaml");
    fs::write(
        &cfg_path,
        "rules:\n  document-start: disable\n  line-length: disable\n  indentation:\n    spaces: 2\n    indent-sequences: consistent\n",
    )
    .unwrap();

    let yaml_path = dir.path().join("input.yaml");
    fs::write(
        &yaml_path,
        "- name: Test nerdctl\n  tasks:\n    - name: Test nerdctl commands\n      loop:\n        - - pull\n          - \"{{ image }}\"\n        - - save\n          - -o\n          - /tmp/hello-world.tar\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(
            yam_code, 0,
            "yamllint should accept compact nested sequence input ({})",
            scenario.label
        );
        assert_eq!(
            ryl_code, 0,
            "ryl should accept compact nested sequence input ({})\n{}",
            scenario.label, ryl_msg
        );
        assert_eq!(ryl_msg, yam_msg, "output mismatch ({})", scenario.label);
    }
}

#[test]
fn root_sequence_of_inline_mappings_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let cfg_path = dir.path().join("cfg.yaml");
    fs::write(
        &cfg_path,
        "rules:\n  document-start: disable\n  indentation:\n    spaces: 2\n",
    )
    .unwrap();

    let yaml_path = dir.path().join("input.yaml");
    fs::write(&yaml_path, "---\n# Foo bar\n- name: Foo\n- name: Bar\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(
            yam_code, 0,
            "yamllint should accept input ({})",
            scenario.label
        );
        assert_eq!(
            ryl_code, 0,
            "ryl should accept input ({})\n{}",
            scenario.label, ryl_msg
        );
        assert_eq!(ryl_msg, yam_msg, "output mismatch ({})", scenario.label);
    }
}

#[test]
fn sequence_entry_mapping_nested_sequence_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();

    let cfg_path = dir.path().join("cfg.yaml");
    fs::write(
        &cfg_path,
        "rules:\n  document-start: disable\n  indentation:\n    spaces: 2\n    indent-sequences: true\n",
    )
    .unwrap();

    let yaml_path = dir.path().join("input.yaml");
    fs::write(&yaml_path, "- key:\n  - nested\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");

    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg_path).arg(&yaml_path);
        let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(
            ryl_code, yam_code,
            "exit mismatch for nested sequence-entry mapping ({})",
            scenario.label
        );
        assert_eq!(
            ryl_msg, yam_msg,
            "diagnostics mismatch for nested sequence-entry mapping ({})",
            scenario.label
        );
    }
}

#[test]
fn readthedocs_style_indentation_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let cfg = dir.path().join("cfg.yaml");
    fs::write(
        &cfg,
        "extends: default\nrules:\n  line-length:\n    max: 110\n",
    )
    .unwrap();

    let input = dir.path().join("readthedocs.yaml");
    fs::write(
        &input,
        "---\nversion: 2\nformats: []\nsphinx:\n    configuration: devel-common/src/docs/rtd-deprecation/conf.py\npython:\n    version: \"3.10\"\n    install:\n        - method: pip\n          path: .\n          extra_requirements:\n              - doc\n    system_packages: true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg).arg(&input);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg).arg(&input);
        let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(
            ryl_code, yam_code,
            "exit mismatch for readthedocs-style indentation ({})",
            scenario.label
        );
        assert_eq!(
            ryl_msg, yam_msg,
            "diagnostics mismatch for readthedocs-style indentation ({})",
            scenario.label
        );
    }
}

#[test]
fn flow_mapping_continuation_indentation_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let cfg = dir.path().join("cfg.yml");
    fs::write(
        &cfg,
        "extends: default\nrules:\n  line-length:\n    max: 110\n",
    )
    .unwrap();

    let input = dir.path().join("input.yml");
    fs::write(
        &input,
        "---\nserializers:\n  - {className: org.example.Serializer,\n     config: {includeTypes: true}}\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg).arg("--strict").arg(&input);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg).arg("--strict").arg(&input);
        let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(
            ryl_code, yam_code,
            "exit mismatch for flow mapping continuation indentation ({})",
            scenario.label
        );
        assert_eq!(
            ryl_msg, yam_msg,
            "diagnostics mismatch for flow mapping continuation indentation ({})",
            scenario.label
        );
    }
}

#[test]
fn multiline_double_quoted_template_indentation_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let cfg = dir.path().join("cfg.yml");
    fs::write(
        &cfg,
        "extends: default\nrules:\n  line-length:\n    max: 110\n",
    )
    .unwrap();

    let input = dir.path().join("input.yml");
    fs::write(
        &input,
        "---\nlogging:\n  log_filename_template:\n    default: \"dag_id={{ ti.dag_id }}/run_id={{ ti.run_id }}/task_id={{ ti.task_id }}/\\\n             {%% if ti.map_index >= 0 %%}map_index={{ ti.map_index }}/{%% endif %%}\\\n             attempt={{ try_number|default(ti.try_number) }}.log\"\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    for scenario in SCENARIOS {
        let mut ryl_cmd = build_ryl_command(exe, scenario.ryl_format);
        ryl_cmd.arg("-c").arg(&cfg).arg("--strict").arg(&input);
        let (ryl_code, ryl_msg) = capture_with_env(ryl_cmd, scenario.envs);

        let mut yam_cmd = build_yamllint_command(scenario.yam_format);
        yam_cmd.arg("-c").arg(&cfg).arg("--strict").arg(&input);
        let (yam_code, yam_msg) = capture_with_env(yam_cmd, scenario.envs);

        assert_eq!(
            ryl_code, yam_code,
            "exit mismatch for multiline quoted template indentation ({})",
            scenario.label
        );
        assert_eq!(
            ryl_msg, yam_msg,
            "diagnostics mismatch for multiline quoted template indentation ({})",
            scenario.label
        );
    }
}
