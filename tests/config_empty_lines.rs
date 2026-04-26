use ryl::config::YamlLintConfig;

#[test]
fn error_on_non_integer_limits() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    max: true\n")
        .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.empty-lines"), "{err}");

    let err =
        YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    max-start: false\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.empty-lines"), "{err}");

    let err =
        YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    max-end: false\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.empty-lines"), "{err}");
}

#[test]
fn error_on_unknown_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    unexpected: 3\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.empty-lines"), "{err}");
}

#[test]
fn error_on_non_string_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    1: 2\n")
        .unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
