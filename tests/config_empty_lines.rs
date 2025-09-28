use ryl::config::YamlLintConfig;

#[test]
fn error_on_non_integer_limits() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    max: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"max\" of \"empty-lines\" should be int"
    );

    let err = YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    max-start: false\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"max-start\" of \"empty-lines\" should be int"
    );

    let err =
        YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    max-end: false\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"max-end\" of \"empty-lines\" should be int"
    );
}

#[test]
fn error_on_unknown_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    unexpected: 3\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"unexpected\" for rule \"empty-lines\""
    );
}

#[test]
fn error_on_non_string_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  empty-lines:\n    1: 2\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"empty-lines\""
    );
}
