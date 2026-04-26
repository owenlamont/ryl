use ryl::config::YamlLintConfig;

#[test]
fn error_on_non_bool_block_mapping_option() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  empty-values:\n    forbid-in-block-mappings: 1\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.empty-values"), "{err}");
}

#[test]
fn error_on_unknown_option() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  empty-values:\n    unsupported: true\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.empty-values"), "{err}");
}

#[test]
fn error_on_non_string_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  empty-values:\n    1: true\n")
        .unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
