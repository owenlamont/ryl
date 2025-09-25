use ryl::config::{RuleLevel, YamlLintConfig};
use ryl::rules::quoted_strings;

#[test]
fn error_when_quote_type_invalid() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    quote-type: bad\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"quote-type\" of \"quoted-strings\" should be in ('any', 'single', 'double')"
    );
}

#[test]
fn error_when_quote_type_not_string() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    quote-type: 1\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"quote-type\" of \"quoted-strings\" should be in ('any', 'single', 'double')"
    );
}

#[test]
fn error_when_required_invalid() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    required: 3\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"required\" of \"quoted-strings\" should be in (True, False, 'only-when-needed')"
    );
}

#[test]
fn error_when_extra_required_not_sequence() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    extra-required: foo\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"extra-required\" of \"quoted-strings\" should only contain values in [<class 'str'>]"
    );
}

#[test]
fn error_when_extra_required_contains_non_string() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    extra-required: [1]\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"extra-required\" of \"quoted-strings\" should only contain values in [<class 'str'>]"
    );
}

#[test]
fn error_when_extra_allowed_contains_non_string() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    extra-allowed: [true]\n")
            .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"extra-allowed\" of \"quoted-strings\" should only contain values in [<class 'str'>]"
    );
}

#[test]
fn error_when_allow_quoted_quotes_not_bool() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    allow-quoted-quotes: 1\n")
            .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"allow-quoted-quotes\" of \"quoted-strings\" should be bool"
    );
}

#[test]
fn error_when_check_keys_not_bool() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    check-keys: 2\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"check-keys\" of \"quoted-strings\" should be bool"
    );
}

#[test]
fn error_when_required_true_and_extra_required() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    extra-required: ['^http']\n")
            .unwrap_err();
    assert_eq!(
        err,
        "invalid config: quoted-strings: cannot use both \"required: true\" and \"extra-required\""
    );
}

#[test]
fn error_when_required_true_and_extra_allowed() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    extra-allowed: ['^http']\n")
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
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    level: warning\n")
        .expect("config with level should be accepted");
    assert_eq!(cfg.rule_level("quoted-strings"), Some(RuleLevel::Warning));
}

#[test]
fn error_when_rule_ignore_contains_non_string_pattern() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    ignore: [1]\n").unwrap_err();
    assert_eq!(err, "invalid config: ignore should contain file patterns");
}

#[test]
fn error_when_unknown_quoted_strings_option() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    unknown: value\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"unknown\" for rule \"quoted-strings\""
    );
}

#[test]
fn error_when_option_key_not_string() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    1: true\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"quoted-strings\""
    );
}

#[test]
fn resolve_required_true_sets_mode() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  quoted-strings:\n    required: true\n")
        .expect("config parses");
    let resolved = quoted_strings::Config::resolve(&cfg);
    let hits = quoted_strings::check("foo: bar\n", &resolved);
    assert_eq!(hits.len(), 1);
}
