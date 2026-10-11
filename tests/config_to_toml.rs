use std::fs;

use ryl::config::{Overrides, discover_config};
use tempfile::tempdir;

#[test]
fn to_toml_includes_ignore_and_locale_and_rules() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(
                "ignore: ['vendor/**']\nlocale: en_US.UTF-8\nrules: { document-start: disable }\n"
                    .to_string(),
            ),
        },
    )
    .unwrap();
    let toml = ctx.config.to_toml_string();
    assert!(toml.contains("exclude = ["));
    assert!(toml.contains("locale = \"en_US.UTF-8\""));
    assert!(toml.contains("document-start = \"disable\""));
}

#[test]
fn to_toml_includes_ignore_from_file_when_present() {
    let td = tempdir().unwrap();
    fs::write(td.path().join(".ignore-list"), "build/**\n").unwrap();
    fs::write(
        td.path().join(".yamllint"),
        "ignore-from-file: .ignore-list\nrules: {}\n",
    )
    .unwrap();
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(td.path().join(".yamllint")),
            config_data: None,
        },
    )
    .unwrap();
    let toml = ctx.config.to_toml_string();
    assert!(toml.contains("exclude-from-file = ["));
}

#[test]
fn to_toml_includes_per_file_ignores_when_present() {
    let td = tempdir().unwrap();
    let cfg = td.path().join(".ryl.toml");
    fs::write(
        &cfg,
        "[lint.rules]\ndocument-start = 'enable'\n[lint.per-file-ignores]\n'values.yaml' = ['document-start']\n",
    )
    .unwrap();
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
    )
    .unwrap();
    let toml = ctx.config.to_toml_string();
    assert!(toml.contains("[lint.per-file-ignores]"));
    assert!(toml.contains("\"values.yaml\" = ["));
    assert!(toml.contains("\"document-start\""));
}

#[test]
fn to_toml_errors_on_null_values() {
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("rules: { custom-rule: { opt: ~ } }\n".to_string()),
        },
    )
    .unwrap_err();
    assert!(err.contains("cannot convert null values to TOML"));
}

#[test]
fn to_toml_errors_on_non_string_mapping_keys() {
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("rules:\n  custom-rule:\n    1: x\n".to_string()),
        },
    )
    .unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"));
}

#[test]
fn to_toml_errors_on_tagged_values() {
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(
                "rules:\n  custom-rule:\n    tagged: !demo value\n".to_string(),
            ),
        },
    )
    .unwrap_err();
    assert!(err.contains("cannot convert this YAML node to TOML"));
}

#[test]
fn to_toml_round_trips_every_fix_policy_rule() {
    let td = tempdir().unwrap();
    let cfg_path = td.path().join(".ryl.toml");
    let rules = [
        "braces",
        "brackets",
        "colons",
        "commas",
        "comments",
        "comments-indentation",
        "document-end",
        "document-start",
        "empty-lines",
        "hyphens",
        "key-ordering",
        "new-line-at-end-of-file",
        "new-lines",
        "quoted-strings",
        "trailing-spaces",
        "truthy",
    ];
    let names = rules.map(|rule| format!("'{rule}'")).join(", ");
    fs::write(
        &cfg_path,
        format!("[lint]\nfixable = ['ALL', {names}]\nunfixable = [{names}]\n"),
    )
    .unwrap();
    let overrides = Overrides {
        config_file: Some(cfg_path.clone()),
        config_data: None,
    };
    let ctx = discover_config(&[], &overrides).unwrap();
    let rendered = ctx.config.to_toml_string();
    let value: toml::Value = toml::from_str(&rendered).unwrap();
    let expected: Vec<_> = rules.map(|rule| toml::Value::String(rule.into())).into();
    assert_eq!(value["lint"]["unfixable"].as_array().unwrap(), &expected);
    let mut fixable = vec![toml::Value::String("ALL".into())];
    fixable.extend(expected);
    assert_eq!(value["lint"]["fixable"].as_array().unwrap(), &fixable);
    fs::write(&cfg_path, rendered).unwrap();
    let reloaded = discover_config(&[], &overrides).unwrap();
    assert_eq!(ctx.config.fix(), reloaded.config.fix());
    for rule in rules {
        assert!(!reloaded.config.fix().allows_rule(rule), "{rule}");
    }
    fs::write(&cfg_path, format!("[lint]\nfixable = [{names}]\n")).unwrap();
    let allowed = discover_config(&[], &overrides).unwrap();
    for rule in rules {
        assert!(allowed.config.fix().allows_rule(rule), "{rule}");
    }
}
