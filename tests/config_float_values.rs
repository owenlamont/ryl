use ryl::config::YamlLintConfig;

#[test]
fn error_on_non_bool_require_numeral() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  float-values:\n    require-numeral-before-decimal: 1\n",
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"require-numeral-before-decimal\" of \"float-values\" should be bool"
    );
}

#[test]
fn error_on_unknown_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  float-values:\n    minimum: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"minimum\" for rule \"float-values\""
    );
}

#[test]
fn error_on_non_string_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  float-values:\n    1: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"float-values\""
    );
}
