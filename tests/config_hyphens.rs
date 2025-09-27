use ryl::config::YamlLintConfig;
use ryl::rules::hyphens::Config;

#[test]
fn rejects_unknown_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  hyphens:\n    unexpected: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"unexpected\" for rule \"hyphens\""
    );
}

#[test]
fn rejects_non_integer_max_spaces_after() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  hyphens:\n    max-spaces-after: []\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"max-spaces-after\" of \"hyphens\" should be int"
    );
}

#[test]
fn resolve_uses_default_when_option_missing() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  hyphens: enable\n").expect("parse config");
    let resolved = Config::resolve(&cfg);
    assert_eq!(resolved.max_spaces_after(), 1);
}

#[test]
fn resolve_reads_configured_value() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  hyphens:\n    max-spaces-after: 4\n")
        .expect("parse config");
    let resolved = Config::resolve(&cfg);
    assert_eq!(resolved.max_spaces_after(), 4);
}

#[test]
fn rejects_non_string_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  hyphens:\n    1: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"hyphens\""
    );
}
