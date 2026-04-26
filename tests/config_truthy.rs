use ryl::config::YamlLintConfig;

#[test]
fn error_when_allowed_values_not_sequence() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    allowed-values: foo\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.truthy"), "{err}");
}

#[test]
fn error_when_allowed_values_has_invalid_entry() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    allowed-values: [foo]\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.truthy"), "{err}");
}

#[test]
fn error_when_allowed_values_contains_non_string() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    allowed-values: [1]\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.truthy"), "{err}");
}

#[test]
fn error_when_check_keys_not_bool() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    check-keys: 1\n")
        .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.truthy"), "{err}");
}

#[test]
fn error_on_unknown_truthy_option() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    unknown: true\n")
        .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.truthy"), "{err}");
}

#[test]
fn error_on_non_string_truthy_option_key() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    1: true\n").unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}

#[test]
fn rule_option_returns_none_for_scalar_rule_value() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  truthy: enable\n")
        .expect("config parses");
    assert!(cfg.rule_option("truthy", "allowed-values").is_none());
}

#[test]
fn rule_option_returns_none_when_rule_missing() {
    let cfg = YamlLintConfig::from_yaml_str("rules: {}\n").expect("config parses");
    assert!(cfg.rule_option("truthy", "allowed-values").is_none());
}
