use std::fs;

use ryl::config::{Overrides, YamlLintConfig, discover_config};
use tempfile::tempdir;

#[test]
fn error_on_unknown_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  trailing-spaces:\n    foo: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"foo\" for rule \"trailing-spaces\""
    );
}

#[test]
fn error_on_invalid_ignore_type() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  trailing-spaces:\n    ignore: 1\n").unwrap_err();
    assert_eq!(err, "invalid config: ignore should contain file patterns");
}

#[test]
fn error_on_invalid_ignore_from_file_type() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  trailing-spaces:\n    ignore-from-file: 1\n")
            .unwrap_err();
    assert_eq!(
        err,
        "invalid config: ignore-from-file should contain filename(s), either as a list or string"
    );
}

#[test]
fn error_on_invalid_rule_ignore_pattern() {
    let cfg = "rules:\n  trailing-spaces:\n    ignore: ['[']\n";
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .unwrap_err();
    assert!(
        err.contains("invalid config: ignore pattern '["),
        "unexpected error message: {err}"
    );
}

#[test]
fn error_on_missing_rule_ignore_from_file() {
    let cfg = "rules:\n  trailing-spaces:\n    ignore-from-file: missing.txt\n";
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .unwrap_err();
    assert!(
        err.contains("failed to read ignore-from-file"),
        "unexpected error message: {err}"
    );
}

#[test]
fn error_on_invalid_rule_ignore_from_file_pattern() {
    let dir = tempdir().unwrap();
    let patterns = dir.path().join("patterns.txt");
    fs::write(&patterns, "[\n").unwrap();
    let config = dir.path().join("config.yml");
    let config_body = format!(
        "rules:\n  trailing-spaces:\n    ignore-from-file: '{}'\n",
        patterns.display().to_string().replace('\'', "''")
    );
    fs::write(&config, config_body).unwrap();

    let err = discover_config(
        &[],
        &Overrides {
            config_file: Some(config),
            config_data: None,
        },
    )
    .unwrap_err();
    assert!(
        err.contains("invalid config: ignore-from-file pattern"),
        "unexpected error message: {err}"
    );
}

#[test]
fn rule_ignore_from_file_skips_blank_lines() {
    let dir = tempdir().unwrap();
    let patterns = dir.path().join("patterns.txt");
    fs::write(&patterns, "\nignored.yaml\n").unwrap();
    let config = dir.path().join("config.yml");
    let config_body = format!(
        "rules:\n  trailing-spaces:\n    ignore-from-file: '{}'\n",
        patterns.display().to_string().replace('\'', "''")
    );
    fs::write(&config, config_body).unwrap();

    discover_config(
        &[],
        &Overrides {
            config_file: Some(config),
            config_data: None,
        },
    )
    .expect("config with blank lines in ignore-from-file should parse");
}

#[test]
fn rule_ignore_with_empty_list_produces_no_matcher() {
    let cfg = "rules:\n  trailing-spaces:\n    ignore: []\n";
    discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("empty ignore list should be accepted");
}

#[test]
fn rule_ignore_from_file_with_only_blank_lines_produces_no_matcher() {
    let dir = tempdir().unwrap();
    let patterns = dir.path().join("patterns.txt");
    fs::write(&patterns, "\n\n").unwrap();
    let config = dir.path().join("config.yml");
    let config_body = format!(
        "rules:\n  trailing-spaces:\n    ignore-from-file: '{}'\n",
        patterns.display().to_string().replace('\'', "''")
    );
    fs::write(&config, config_body).unwrap();

    discover_config(
        &[],
        &Overrides {
            config_file: Some(config),
            config_data: None,
        },
    )
    .expect("blank ignore-from-file should be accepted");
}
