use ryl::config::YamlLintConfig;

#[test]
fn error_on_non_bool_block_mapping_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  empty-values:\n    forbid-in-block-mappings: 1\n")
            .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"forbid-in-block-mappings\" of \"empty-values\" should be bool"
    );
}

#[test]
fn error_on_unknown_option() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  empty-values:\n    unsupported: true\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"unsupported\" for rule \"empty-values\""
    );
}

#[test]
fn error_on_non_string_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  empty-values:\n    1: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"empty-values\""
    );
}
