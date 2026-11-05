use std::fs;
use std::process::Command;

use tempfile::tempdir;

use crate::common;
use common::cli::run;

fn config_error(name: &str, config: &str) -> String {
    let td = tempdir().unwrap();
    let cfg = td.path().join(name);
    fs::write(&cfg, config).unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    let (code, _, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("check")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 2, "stderr={stderr}");
    assert_eq!(stderr.lines().count(), 1, "stderr={stderr}");
    stderr.trim_end().to_owned()
}

#[test]
fn toml_syntax_error_is_one_line_with_location() {
    assert_eq!(
        config_error("bad.toml", "[rules\n"),
        "failed to parse config data: TOML parse error at line 1, column 7: \
         unclosed table, expected `]`"
    );
}

#[test]
fn pyproject_syntax_error_counts_columns_in_chars() {
    assert_eq!(
        config_error("pyproject.toml", "[tool.ryl]\nx = \"é\" é\n"),
        "failed to parse config data: TOML parse error at line 2, column 9: \
         unexpected key or value, expected newline, `#`"
    );
}

#[test]
fn yaml_config_type_error_keeps_key_path_on_one_line() {
    assert_eq!(
        config_error("bad.yaml", "rules:\n  indentation:\n    spaces: x\n"),
        "invalid config: an integer or \"consistent\" in `rules.indentation.spaces`"
    );
}

#[test]
fn yaml_unknown_rule_option_names_rule_and_option() {
    assert_eq!(
        config_error("bad.yaml", "rules: {truthy: {future-option: true}}\n"),
        "invalid config: unknown field `future-option`, expected `allowed-values` \
         or `check-keys` in `rules.truthy`"
    );
}

#[test]
fn toml_unknown_rule_option_names_rule_and_option() {
    assert_eq!(
        config_error("bad.toml", "[lint.rules.truthy]\nfuture-option = true\n"),
        "invalid config: TOML parse error at line 2, column 1: unknown field \
         `future-option`, expected `allowed-values` or `check-keys` in `lint.rules.truthy`"
    );
}

#[test]
fn yaml_invalid_rule_option_value_names_option_and_expected_type() {
    assert_eq!(
        config_error("bad.yaml", "rules: {truthy: {check-keys: maybe}}\n"),
        "invalid config: invalid type: string \"maybe\", expected a boolean \
         in `rules.truthy.check-keys`"
    );
}

#[test]
fn toml_invalid_rule_option_value_names_option_and_expected_type() {
    assert_eq!(
        config_error("bad.toml", "[lint.rules.truthy]\ncheck-keys = \"maybe\"\n"),
        "invalid config: TOML parse error at line 2, column 14: invalid type: \
         string \"maybe\", expected a boolean in `lint.rules.truthy.check-keys`"
    );
}

#[test]
fn newline_in_a_config_key_stays_escaped() {
    let err = config_error("bad.toml", "[output]\n\"a\\n::error::x\" = 1\n");
    assert!(err.contains("unknown field `a\\u{a}::error::x`"), "{err}");
    assert!(err.ends_with("in `output`"), "{err}");
}

#[test]
fn pattern_file_errors_name_the_selected_toml_key() {
    for (config, key) in [
        (
            "exclude-from-file = 'missing.ignore'\n",
            "exclude-from-file",
        ),
        ("ignore-from-file = 'missing.ignore'\n", "ignore-from-file"),
        (
            "ignore-from-file = 'old.ignore'\nexclude-from-file = 'missing.ignore'\n",
            "exclude-from-file",
        ),
    ] {
        let err = config_error("bad.toml", config);
        assert!(err.contains(&format!("failed to read {key} ")), "{err}");
    }

    let td = tempdir().unwrap();
    fs::write(td.path().join("invalid.ignore"), "[\n").unwrap();
    let cfg = td.path().join("bad.toml");
    fs::write(&cfg, "exclude-from-file = 'invalid.ignore'\n").unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    let (code, _, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("check")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("invalid config: exclude-from-file pattern"),
        "{stderr}"
    );
}
