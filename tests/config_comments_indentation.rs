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

#[test]
fn accepts_boolean_toggle() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  comments-indentation: false\n")
        .expect("config should parse");
    assert!(cfg.rule_level("comments-indentation").is_none());
}
