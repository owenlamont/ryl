use ryl::config::YamlLintConfig;

#[test]
fn error_on_non_bool_for_forbid_merge_keys() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  key-duplicates:\n    forbid-duplicated-merge-keys: 1\n",
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"forbid-duplicated-merge-keys\" of \"key-duplicates\" should be bool"
    );
}

#[test]
fn error_on_unknown_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  key-duplicates:\n    foo: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"foo\" for rule \"key-duplicates\""
    );
}

#[test]
fn error_on_non_string_option_key() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  key-duplicates:\n    1: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"key-duplicates\""
    );
}
