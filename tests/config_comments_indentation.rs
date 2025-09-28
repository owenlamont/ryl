use ryl::config::YamlLintConfig;

#[test]
fn error_on_unknown_option() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  comments-indentation:\n    foo: true\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"foo\" for rule \"comments-indentation\""
    );
}
