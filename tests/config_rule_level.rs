use ryl::config::{Overrides, RuleLevel, YamlLintConfig, discover_config};

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
    assert!(
        err.contains("level should be \"error\" or \"warning\""),
        "unexpected error message: {err}"
    );
}

#[test]
fn invalid_level_value_errors() {
    let cfg = r#"
rules:
  new-line-at-end-of-file:
    level: invalid
"#;
    let err = YamlLintConfig::from_yaml_str(cfg).expect_err("invalid");
    assert!(
        err.contains("level should be \"error\" or \"warning\""),
        "unexpected error message: {err}"
    );
}

#[test]
fn invalid_rule_value_type_errors() {
    let cfg = r#"
rules:
  new-line-at-end-of-file: 42
"#;
    let err = YamlLintConfig::from_yaml_str(cfg).expect_err("invalid");
    assert!(
        err.contains("should be 'enable', 'disable', or a mapping"),
        "unexpected error message: {err}"
    );
}

#[test]
fn invalid_rule_value_string_errors() {
    let cfg = r#"
rules:
  new-line-at-end-of-file: other
"#;
    let err = YamlLintConfig::from_yaml_str(cfg).expect_err("invalid");
    assert!(
        err.contains("should be 'enable', 'disable', or a mapping"),
        "unexpected error message: {err}"
    );
}
