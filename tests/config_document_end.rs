use ryl::config::YamlLintConfig;
use ryl::rules::document_end::Config;

#[test]
fn resolve_defaults_to_present_true() {
    let cfg =
        YamlLintConfig::from_yaml_str("rules:\n  document-end: enable\n").expect("config parses");
    let rule_cfg = Config::resolve(&cfg);
    assert!(rule_cfg.requires_marker(), "default should require marker");
}

#[test]
fn resolve_reads_present_override() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  document-end:\n    present: false\n")
        .expect("config parses");
    let rule_cfg = Config::resolve(&cfg);
    assert!(
        !rule_cfg.requires_marker(),
        "override should disable marker"
    );
}

#[test]
fn invalid_present_value_is_rejected() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  document-end:\n    present: 1\n")
        .expect_err("invalid bool should fail");
    assert_eq!(
        err,
        "invalid config: option \"present\" of \"document-end\" should be bool"
    );
}

#[test]
fn unknown_option_is_rejected() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  document-end:\n    extra: true\n")
        .expect_err("unknown option should fail");
    assert_eq!(
        err,
        "invalid config: unknown option \"extra\" for rule \"document-end\""
    );
}

#[test]
fn non_string_option_key_is_rejected() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  document-end:\n    1: true\n")
        .expect_err("non string key should fail");
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"document-end\""
    );
}
