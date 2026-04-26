use ryl::config::YamlLintConfig;

#[test]
fn error_on_non_bool_for_forbid_implicit() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  octal-values:\n    forbid-implicit-octal: 1\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.octal-values"), "{err}");
}

#[test]
fn error_on_unknown_option() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  octal-values:\n    foo: true\n")
        .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.octal-values"), "{err}");
}

#[test]
fn error_on_non_string_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  octal-values:\n    1: true\n")
        .unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
