use ryl::config::{RuleLevel, YamlLintConfig};
use ryl::rules::quoted_strings;

#[test]
fn error_when_quote_type_invalid() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    quote-type: bad\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn quote_type_consistent_is_accepted() {
    YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    quote-type: consistent\n",
    )
    .expect("consistent quote type should be accepted");
}

#[test]
fn error_when_quote_type_not_string() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    quote-type: 1\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_required_invalid() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    required: 3\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_required_is_null() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    required: null\n",
    )
    .unwrap_err();
    assert!(err.contains("cannot convert null values to TOML"), "{err}");
}

#[test]
fn error_when_extra_required_not_sequence() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    extra-required: foo\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_extra_required_contains_non_string() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    extra-required: [1]\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_extra_allowed_contains_non_string() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    extra-allowed: [true]\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_allow_quoted_quotes_not_bool() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    allow-quoted-quotes: 1\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_allow_double_quotes_for_escaping_is_in_yaml_config() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    allow-double-quotes-for-escaping: true\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_check_keys_not_bool() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    check-keys: 2\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_required_true_and_extra_required() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    extra-required: ['^http']\n",
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid config: quoted-strings: cannot use both \"required: true\" and \"extra-required\""
    );
}

#[test]
fn error_when_required_true_and_extra_allowed() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    extra-allowed: ['^http']\n",
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid config: quoted-strings: cannot use both \"required: true\" and \"extra-allowed\""
    );
}

#[test]
fn error_when_required_false_and_extra_allowed() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    required: false\n    extra-allowed: ['^http']\n",
    )
    .unwrap_err();
    assert_eq!(
        err,
        "invalid config: quoted-strings: cannot use both \"required: false\" and \"extra-allowed\""
    );
}

#[test]
fn error_when_extra_required_regex_invalid() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    required: false\n    extra-required: ['[']\n",
    )
    .unwrap_err();
    assert!(
        err.starts_with(
            "invalid config: regex \"[\" in option \"extra-required\" of \"quoted-strings\" is invalid:"
        ),
        "unexpected message: {err}"
    );
}

#[test]
fn allows_level_option_to_pass_validation() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    level: warning\n",
    )
    .expect("config with level should be accepted");
    assert_eq!(cfg.rule_level("quoted-strings"), Some(RuleLevel::Warning));
}

#[test]
fn error_when_rule_ignore_contains_non_string_pattern() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    ignore: [1]\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_unknown_quoted_strings_option() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    unknown: value\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.quoted-strings"), "{err}");
}

#[test]
fn error_when_option_key_not_string() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    1: true\n")
        .unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}

#[test]
fn resolve_required_true_sets_mode() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  quoted-strings:\n    required: true\n",
    )
    .expect("config parses");
    let resolved = quoted_strings::Config::resolve(&cfg);
    let hits = quoted_strings::check("foo: bar\n", &resolved);
    assert_eq!(hits.len(), 1);
}
