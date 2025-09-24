use ryl::config::YamlLintConfig;

#[test]
fn error_when_allowed_values_not_sequence() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    allowed-values: foo\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"allowed-values\" of \"truthy\" should only contain values in ['YES', 'Yes', 'yes', 'NO', 'No', 'no', 'TRUE', 'True', 'true', 'FALSE', 'False', 'false', 'ON', 'On', 'on', 'OFF', 'Off', 'off']"
    );
}

#[test]
fn error_when_allowed_values_has_invalid_entry() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    allowed-values: [foo]\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"allowed-values\" of \"truthy\" should only contain values in ['YES', 'Yes', 'yes', 'NO', 'No', 'no', 'TRUE', 'True', 'true', 'FALSE', 'False', 'false', 'ON', 'On', 'on', 'OFF', 'Off', 'off']"
    );
}

#[test]
fn error_when_allowed_values_contains_non_string() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    allowed-values: [1]\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"allowed-values\" of \"truthy\" should only contain values in ['YES', 'Yes', 'yes', 'NO', 'No', 'no', 'TRUE', 'True', 'true', 'FALSE', 'False', 'false', 'ON', 'On', 'on', 'OFF', 'Off', 'off']"
    );
}

#[test]
fn error_when_check_keys_not_bool() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    check-keys: 1\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"check-keys\" of \"truthy\" should be bool"
    );
}

#[test]
fn error_on_unknown_truthy_option() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    unknown: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"unknown\" for rule \"truthy\""
    );
}

#[test]
fn error_on_non_string_truthy_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  truthy:\n    1: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"truthy\""
    );
}

#[test]
fn rule_option_returns_none_for_scalar_rule_value() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  truthy: enable\n").expect("config parses");
    assert!(cfg.rule_option("truthy", "allowed-values").is_none());
}

#[test]
fn rule_option_returns_none_when_rule_missing() {
    let cfg = YamlLintConfig::from_yaml_str("rules: {}\n").expect("config parses");
    assert!(cfg.rule_option("truthy", "allowed-values").is_none());
}
