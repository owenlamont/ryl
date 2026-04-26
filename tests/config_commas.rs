use ryl::config::YamlLintConfig;
use ryl::rules::commas::Config;

#[test]
fn rejects_unknown_option() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  commas:\n    unexpected: 1\n")
        .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.commas"), "{err}");
}

#[test]
fn rejects_non_integer_max_spaces_before() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  commas:\n    max-spaces-before: []\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.commas"), "{err}");
}

#[test]
fn rejects_non_integer_min_spaces_after() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  commas:\n    min-spaces-after: true\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.commas"), "{err}");
}

#[test]
fn rejects_non_integer_max_spaces_after() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  commas:\n    max-spaces-after: []\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.commas"), "{err}");
}

#[test]
fn rejects_non_string_option_key() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  commas:\n    ? [foo, bar]\n    : 1\n")
            .unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}

#[test]
fn resolve_uses_default_values() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  commas: enable\n")
        .expect("parse config");
    let resolved = Config::resolve(&cfg);
    assert_eq!(resolved.max_spaces_before(), 0);
    assert_eq!(resolved.min_spaces_after(), 1);
    assert_eq!(resolved.max_spaces_after(), 1);
}

#[test]
fn resolve_reads_configured_values() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  commas:\n    max-spaces-before: 2\n    min-spaces-after: 0\n    max-spaces-after: -1\n",
    )
    .expect("parse config");
    let resolved = Config::resolve(&cfg);
    assert_eq!(resolved.max_spaces_before(), 2);
    assert_eq!(resolved.min_spaces_after(), 0);
    assert_eq!(resolved.max_spaces_after(), -1);
}
