use ryl::config::YamlLintConfig;

#[test]
fn rejects_unknown_option() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  line-length:\n    unexpected: true\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"unexpected\" for rule \"line-length\""
    );
}

#[test]
fn rejects_non_integer_max() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  line-length:\n    max: []\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"max\" of \"line-length\" should be int"
    );
}

#[test]
fn rejects_non_boolean_allow_words() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  line-length:\n    allow-non-breakable-words: []\n",
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"allow-non-breakable-words\" of \"line-length\" should be bool"
    );
}

#[test]
fn rejects_non_boolean_allow_inline() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  line-length:\n    allow-non-breakable-inline-mappings: 1\n",
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"allow-non-breakable-inline-mappings\" of \"line-length\" should be bool"
    );
}

#[test]
fn accepts_valid_configuration() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  line-length:\n    max: 100\n    allow-non-breakable-words: false\n    allow-non-breakable-inline-mappings: true\n",
    )
    .expect("configuration should parse");
    assert!(cfg.rule_names().iter().any(|name| name == "line-length"));
}

#[test]
fn rejects_non_string_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  line-length:\n    1: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"line-length\""
    );
}
