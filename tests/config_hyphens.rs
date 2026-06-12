use ryl::config::YamlLintConfig;
use ryl::rules::hyphens::{self, Config};

#[test]
fn rejects_unknown_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  hyphens:\n    unexpected: true\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.hyphens"), "{err}");
}

#[test]
fn rejects_non_integer_max_spaces_after() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  hyphens:\n    max-spaces-after: []\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.hyphens"), "{err}");
}

#[test]
fn resolve_uses_default_when_option_missing() {
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  hyphens: enable\n")
        .expect("parse config");
    let resolved = Config::resolve(&cfg);
    assert_eq!(resolved.max_spaces_after(), 1);
}

#[test]
fn resolve_reads_configured_value() {
    let cfg =
        YamlLintConfig::from_yaml_str("rules:\n  hyphens:\n    max-spaces-after: 4\n")
            .expect("parse config");
    let resolved = Config::resolve(&cfg);
    assert_eq!(resolved.max_spaces_after(), 4);
}

#[test]
fn resolve_reads_dash_on_own_line_from_toml() {
    let cfg =
        YamlLintConfig::from_toml_str("[rules.hyphens]\ndash-on-own-line = true\n")
            .expect("parse TOML config");
    let resolved = Config::resolve(&cfg);
    // No bool getter is exposed; assert the resolved config drives the check.
    let diagnostics = hyphens::check("items:\n  - name: web\n", &resolved);
    assert_eq!(diagnostics.len(), 1, "option should enable the check");
}

#[test]
fn dash_on_own_line_rejected_in_yaml_config() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  hyphens:\n    dash-on-own-line: true\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.hyphens"), "{err}");
}

#[test]
fn rejects_non_string_option_key() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  hyphens:\n    1: true\n").unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
