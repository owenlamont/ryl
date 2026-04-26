use ryl::config::YamlLintConfig;

#[test]
fn error_on_unknown_option() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  comments-indentation:\n    foo: true\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.comments-indentation"), "{err}");
}

#[test]
fn accepts_boolean_toggle() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  comments-indentation: false\n")
        .expect("config should parse");
    assert!(cfg.rule_level("comments-indentation").is_none());
}
