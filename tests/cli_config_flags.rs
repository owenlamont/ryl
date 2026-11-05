use std::fs;
use std::process::Command;

use tempfile::tempdir;

use crate::common;
use common::cli::run;

#[test]
fn yaml_config_boolean_spellings_control_key_checks() {
    let td = tempdir().unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "yes: true\n").unwrap();
    for (spellings, expected) in [
        (
            [
                "yes", "Yes", "YES", "on", "On", "ON", "true", "True", "TRUE",
            ],
            1,
        ),
        (
            [
                "no", "No", "NO", "off", "Off", "OFF", "false", "False", "FALSE",
            ],
            0,
        ),
    ] {
        for spelling in spellings {
            let config = format!("rules:\n  truthy:\n    check-keys: {spelling}\n");
            let (code, out, err) = run(common::cli::ryl(td.path())
                .args(["check", "-d", &config])
                .arg(&file));
            assert_eq!(code, expected, "{spelling}: {out}\n{err}");
        }
    }
}

#[test]
fn yaml_config_legacy_octal_controls_line_length() {
    let td = tempdir().unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "name: abcdefghijk\n").unwrap();
    let (code, out, err) = run(common::cli::ryl(td.path())
        .args(["check", "-d", "rules:\n  line-length:\n    max: 017\n"])
        .arg(&file));
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(format!("{out}{err}").contains("17 > 15"), "{out}\n{err}");
}

#[test]
fn yaml_config_preserves_strings_and_rejects_invalid_option_types() {
    let td = tempdir().unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    for (rule, option, spellings) in [
        (
            "truthy",
            "check-keys",
            &["'no'", "\"yes\"", "y", "n", "yEs", "~"][..],
        ),
        (
            "line-length",
            "max",
            &["'017'", "08", "1e3", "1.0e+3", "1_0.0", ".inf", "~"][..],
        ),
    ] {
        for spelling in spellings {
            let config = format!("rules:\n  {rule}:\n    {option}: {spelling}\n");
            let (code, out, err) = run(common::cli::ryl(td.path())
                .args(["check", "-d", &config])
                .arg(&file));
            assert_eq!(code, 2, "{spelling}: {out}\n{err}");
        }
    }
}

#[test]
fn yaml_config_resolves_types_inside_string_lists_and_mapping_keys() {
    let td = tempdir().unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    for (config, expected) in [
        ("ignore: [1e3, 08, 2001-1-1]\nrules: {truthy: enable}\n", 0),
        (
            "ignore: [0o1.0, 0o+7, --0o17]\nrules: {truthy: enable}\n",
            0,
        ),
        (
            "ignore: ['yes', '2001-01-01']\nrules: {truthy: enable}\n",
            0,
        ),
        ("ignore: [yes]\nrules: {truthy: enable}\n", 2),
        ("ignore: [2001-01-01]\nrules: {truthy: enable}\n", 2),
        ("rules: {on: enable}\n", 2),
        ("rules: {truthy: {on: false}}\n", 2),
    ] {
        let (code, out, err) = run(common::cli::ryl(td.path())
            .args(["check", "-d", config])
            .arg(&file));
        assert_eq!(code, expected, "{config}: {out}\n{err}");
    }
}

#[test]
fn yaml_config_boolean_resolution_is_shared_by_every_source() {
    for source in ["inline", "file", "project", "env", "user", "extends"] {
        let td = tempdir().unwrap();
        let root = td.path();
        let file = root.join("a.yaml");
        fs::write(&file, "yes: true\n").unwrap();
        let config = "rules:\n  truthy:\n    check-keys: no\n";
        let path = root.join("base.yaml");
        fs::write(&path, config).unwrap();
        let mut cmd = common::cli::ryl(root);
        cmd.current_dir(root)
            .env_remove("YAMLLINT_CONFIG_FILE")
            .env("XDG_CONFIG_HOME", root)
            .arg("check");
        match source {
            "inline" => {
                cmd.args(["-d", config]);
            }
            "file" => {
                cmd.arg("-c").arg(&path);
            }
            "project" => {
                fs::write(root.join(".yamllint"), config).unwrap();
            }
            "env" => {
                cmd.env("YAMLLINT_CONFIG_FILE", &path);
            }
            "user" => {
                fs::create_dir(root.join("yamllint")).unwrap();
                fs::write(root.join("yamllint/config"), config).unwrap();
            }
            "extends" => {
                cmd.args(["-d", "extends: base.yaml\n"]);
            }
            _ => unreachable!(),
        }
        let (code, out, err) = run(cmd.arg(&file));
        assert_eq!(code, 0, "{source}: {out}\n{err}");
    }
}

#[test]
fn config_file_ignores_docs_globally() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("docs/ignored.yaml"), "x: 0\n").unwrap();

    // `--list-files` answers a file-selection query and is exempt from the
    // "no rules enabled" lint check, so a rule-less config is fine here.
    let cfg = root.join("cfg.yml");
    fs::write(&cfg, "ignore: ['docs/**']\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-c")
        .arg(&cfg)
        .arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.contains("a.yaml"));
    assert!(!out.contains("docs/ignored.yaml"));
}

#[test]
fn config_data_yaml_files_only_lists_dot_yamllint_yml() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join(".yamllint.yml"), "rules: {}\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg("yaml-files: ['**/.yamllint.yml']\n")
        .arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.contains(".yamllint.yml"));
    assert!(!out.contains("a.yaml"));
}

#[test]
fn config_enabling_no_rules_is_rejected_loudly() {
    let td = tempdir().unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    // A config that turns every rule off lints nothing; ryl rejects it (stricter than
    // yamllint, which silently accepts a rule-less config) rather than exit 0 silently.
    for data in ["rules: {}\n", "rules:\n  document-start: disable\n"] {
        let (code, _out, err) = run(Command::new(exe).arg("-d").arg(data).arg(&file));
        assert_eq!(code, 2, "a rule-less config must error: {err}");
        assert!(
            err.contains("enables no rules") && err.contains("--enable ALL"),
            "expected the no-rules error suggesting --enable: {err}"
        );
    }

    // `--list-files` is exempt: it answers a file query, it does not lint.
    let (code, _out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg("rules: {}\n")
        .arg(&file));
    assert_eq!(
        code, 0,
        "--list-files must not require enabled rules: {err}"
    );
}

#[test]
fn format_strict_no_warnings_are_accepted() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-f")
        .arg("standard")
        .arg("-s")
        .arg("--no-warnings")
        .arg(td.path()));
    assert_eq!(code, 0, "expected success: {err}");
}
