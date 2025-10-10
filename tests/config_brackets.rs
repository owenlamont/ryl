use ryl::config::YamlLintConfig;
use ryl::rules::brackets::{Config as BracketsConfig, Forbid};

fn parse_config(input: &str) -> YamlLintConfig {
    YamlLintConfig::from_yaml_str(input).expect("config should parse")
}

#[test]
fn brackets_options_are_parsed() {
    let cfg = parse_config(
        "rules:\n  brackets:\n    forbid: non-empty\n    min-spaces-inside: 1\n    max-spaces-inside: 2\n    min-spaces-inside-empty: 3\n    max-spaces-inside-empty: 4\n",
    );

    let rule_cfg = BracketsConfig::resolve(&cfg);
    assert_eq!(rule_cfg.forbid(), Forbid::NonEmpty);
    assert_eq!(rule_cfg.min_spaces_inside(), 1);
    assert_eq!(rule_cfg.max_spaces_inside(), 2);
    assert_eq!(rule_cfg.effective_min_empty(), 3);
    assert_eq!(rule_cfg.effective_max_empty(), 4);
}

#[test]
fn brackets_empty_overrides_fallback_to_main_values() {
    let cfg =
        parse_config("rules:\n  brackets:\n    min-spaces-inside: 2\n    max-spaces-inside: 3\n");

    let rule_cfg = BracketsConfig::resolve(&cfg);
    assert_eq!(rule_cfg.effective_min_empty(), 2);
    assert_eq!(rule_cfg.effective_max_empty(), 3);
}

#[test]
fn brackets_invalid_forbid_value_is_rejected() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  brackets:\n    forbid: maybe\n")
        .expect_err("config should fail");
    assert!(
        err.contains("option \"forbid\" of \"brackets\" should be bool or \"non-empty\""),
        "unexpected error: {err}"
    );
}

#[test]
fn brackets_invalid_numeric_option_is_rejected() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  brackets:\n    min-spaces-inside: foo\n")
        .expect_err("config should fail");
    assert!(
        err.contains("option \"min-spaces-inside\" of \"brackets\" should be int"),
        "unexpected error: {err}"
    );
}

#[test]
fn brackets_invalid_max_option_is_rejected() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  brackets:\n    max-spaces-inside: foo\n")
        .expect_err("config should fail");
    assert!(
        err.contains("option \"max-spaces-inside\" of \"brackets\" should be int"),
        "unexpected error: {err}"
    );
}

#[test]
fn brackets_invalid_empty_max_option_is_rejected() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  brackets:\n    max-spaces-inside-empty: foo\n")
            .expect_err("config should fail");
    assert!(
        err.contains("option \"max-spaces-inside-empty\" of \"brackets\" should be int"),
        "unexpected error: {err}"
    );
}

#[test]
fn brackets_invalid_empty_min_option_is_rejected() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  brackets:\n    min-spaces-inside-empty: foo\n")
            .expect_err("config should fail");
    assert!(
        err.contains("option \"min-spaces-inside-empty\" of \"brackets\" should be int"),
        "unexpected error: {err}"
    );
}

#[test]
fn brackets_unknown_string_option_is_rejected() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  brackets:\n    unexpected-option: true\n")
        .expect_err("config should fail");
    assert!(
        err.contains("invalid config: unknown option \"unexpected-option\" for rule \"brackets\""),
        "unexpected error: {err}"
    );
}

#[test]
fn brackets_non_string_key_reports_error() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  brackets:\n    1: true\n")
        .expect_err("config should fail");
    assert!(
        err.contains("invalid config: unknown option \"1\" for rule \"brackets\""),
        "unexpected error: {err}"
    );
}

#[test]
fn forbid_false_maps_to_none() {
    let cfg = parse_config("rules:\n  brackets:\n    forbid: false\n");

    let rule_cfg = BracketsConfig::resolve(&cfg);
    assert_eq!(rule_cfg.forbid(), Forbid::None);
}
