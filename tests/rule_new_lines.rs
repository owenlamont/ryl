use ryl::config::YamlLintConfig;
use ryl::rules::new_lines::{self, Config, LineKind};

#[test]
fn default_config_uses_unix() {
    let cfg =
        YamlLintConfig::from_yaml_str("rules:\n  new-lines: enable\n").expect("config parses");
    let resolved = Config::resolve(&cfg);
    assert_eq!(resolved.kind, LineKind::Unix);
}

#[test]
fn platform_newline_can_be_overridden_for_tests() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  new-lines:\n    type: platform\n")
        .expect("config parses");
    let resolved = Config::resolve(&cfg);
    let violation = new_lines::check("key: 1\n", resolved, "\r\n");
    let err = violation.expect("should report mismatch");
    assert_eq!(err.line, 1);
    assert_eq!(err.column, 7);
    assert_eq!(err.message, "wrong new line character: expected \\r\\n");
}

#[test]
fn check_reports_column_in_chars() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  new-lines:\n    type: dos\n")
        .expect("config parses");
    let resolved = Config::resolve(&cfg);
    let violation = new_lines::check("héllo\n", resolved, new_lines::platform_newline());
    let err = violation.expect("should report mismatch");
    assert_eq!(err.column, 6, "unicode aware column");
}

#[test]
fn check_returns_none_when_newlines_match() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  new-lines:\n    type: unix\n")
        .expect("config parses");
    let resolved = Config::resolve(&cfg);
    let outcome = new_lines::check("key: value\n", resolved, new_lines::platform_newline());
    assert!(outcome.is_none());
}

#[test]
fn resolve_defaults_when_rule_missing() {
    let cfg = YamlLintConfig::from_yaml_str("rules: {}\n").expect("config parses");
    let resolved = Config::resolve(&cfg);
    assert_eq!(resolved.kind, LineKind::Unix);
}
