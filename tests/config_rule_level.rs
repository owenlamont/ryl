use std::fs;

use ryl::config::{Overrides, RuleLevel, YamlLintConfig, discover_config};
use tempfile::tempdir;

#[test]
fn unknown_rule_name_is_rejected() {
    // ryl does not support custom/unrecognised rules (matching yamllint): an unknown
    // rule name (including a typo of a real one) is a config error, not a silent
    // no-op. Covers both the YAML and TOML config paths.
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("rules:\n  not-a-real-rule: enable\n".into()),
        },
    )
    .expect_err("an unknown rule name must be rejected");
    assert_eq!(err, "invalid config: no such rule: \"not-a-real-rule\"");

    let td = tempdir().unwrap();
    let cfg = td.path().join(".ryl.toml");
    fs::write(&cfg, "[rules]\ntariling-spaces = \"enable\"\n").unwrap();
    let err = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
    )
    .expect_err("a typo'd rule name must be rejected, not silently ignored");
    assert!(err.contains("no such rule: \"tariling-spaces\""), "{err}");
}

#[test]
fn unknown_rule_with_float_and_datetime_options_is_rejected() {
    // An unknown rule's option values still flow through TOML->YAML scalar
    // conversion (including float and datetime) before the rule name is checked;
    // the rejection must win regardless of those option value types.
    let td = tempdir().unwrap();
    let cfg = td.path().join(".ryl.toml");
    fs::write(
        &cfg,
        "[rules.made-up-rule]\nratio = 1.5\nstamp = 1979-05-27T07:32:00Z\n",
    )
    .unwrap();
    let err = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
    )
    .expect_err("an unknown rule with float/datetime options must still be rejected");
    assert!(err.contains("no such rule: \"made-up-rule\""), "{err}");
}

#[test]
fn rule_level_returns_none_for_disable() {
    let cfg = r#"
rules:
  new-line-at-end-of-file: disable
"#;
    let context = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("config");
    assert!(
        context
            .config
            .rule_level("new-line-at-end-of-file")
            .is_none()
    );
}

#[test]
fn rule_level_parses_warning_mapping() {
    let cfg = r#"
rules:
  new-line-at-end-of-file:
    level: warning
"#;
    let context = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("config");
    assert_eq!(
        context
            .config
            .rule_level("new-line-at-end-of-file")
            .expect("level"),
        RuleLevel::Warning
    );
}

#[test]
fn rule_level_defaults_to_error_for_enable() {
    let cfg = r#"
rules:
  new-line-at-end-of-file: enable
"#;
    let context = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("config");
    assert_eq!(
        context
            .config
            .rule_level("new-line-at-end-of-file")
            .expect("level"),
        RuleLevel::Error
    );
}

#[test]
fn rule_level_defaults_to_error_when_mapping_missing_level() {
    let cfg = r#"
rules:
  new-line-at-end-of-file: {}
"#;
    let context = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("config");
    assert_eq!(
        context
            .config
            .rule_level("new-line-at-end-of-file")
            .expect("level"),
        RuleLevel::Error
    );
}

#[test]
fn invalid_level_not_string_errors() {
    let cfg = r#"
rules:
  new-line-at-end-of-file:
    level: [1]
"#;
    let err = YamlLintConfig::from_yaml_str(cfg).expect_err("invalid");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.new-line-at-end-of-file"), "{err}");
}

#[test]
fn invalid_level_value_errors() {
    let cfg = r#"
rules:
  new-line-at-end-of-file:
    level: invalid
"#;
    let err = YamlLintConfig::from_yaml_str(cfg).expect_err("invalid");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.new-line-at-end-of-file"), "{err}");
}

#[test]
fn invalid_rule_value_type_errors() {
    let cfg = r#"
rules:
  new-line-at-end-of-file: 42
"#;
    let err = YamlLintConfig::from_yaml_str(cfg).expect_err("invalid");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.new-line-at-end-of-file"), "{err}");
}

#[test]
fn invalid_rule_value_string_errors() {
    let cfg = r#"
rules:
  new-line-at-end-of-file: other
"#;
    let err = YamlLintConfig::from_yaml_str(cfg).expect_err("invalid");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.new-line-at-end-of-file"), "{err}");
}

#[test]
fn custom_yaml_rule_with_non_string_mapping_keys_is_rejected() {
    let cfg = r#"
rules:
  custom-rule:
    1: value
"#;
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("non-string keys should fail typed YAML parsing");
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
