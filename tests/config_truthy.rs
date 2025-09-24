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
