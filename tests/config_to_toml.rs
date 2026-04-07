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
    let toml = ctx.config.to_toml_string().unwrap();
    assert!(toml.contains("ignore = ["));
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
    let toml = ctx.config.to_toml_string().unwrap();
    assert!(toml.contains("ignore-from-file = ["));
}

#[test]
fn to_toml_errors_on_null_values() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("rules: { custom-rule: { opt: ~ } }\n".to_string()),
        },
    )
    .unwrap();
    let err = ctx.config.to_toml_string().unwrap_err();
    assert!(err.contains("cannot convert null values to TOML"));
}

#[test]
fn to_toml_converts_scalar_sequence_and_mapping_values() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(
                "rules:\n  custom-rule:\n    flag: true\n    count: 3\n    ratio: 1.5\n    list: [1, false]\n    nested: { child: 1 }\n"
                    .to_string(),
            ),
        },
    )
    .unwrap();
    let toml = ctx.config.to_toml_string().unwrap();
    assert!(toml.contains("flag = true"));
    assert!(toml.contains("count = 3"));
    assert!(toml.contains("ratio = 1.5"));
    assert!(toml.contains("list = ["));
    assert!(toml.contains("false"));
    assert!(toml.contains("[rules.custom-rule.nested]"));
}

#[test]
fn to_toml_errors_on_non_string_mapping_keys() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("rules:\n  custom-rule:\n    1: x\n".to_string()),
        },
    )
    .unwrap();
    let err = ctx.config.to_toml_string().unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"));
}

#[test]
fn to_toml_errors_on_tagged_values() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(
                "rules:\n  custom-rule:\n    tagged: !demo value\n".to_string(),
            ),
        },
    )
    .unwrap();
    let err = ctx.config.to_toml_string().unwrap_err();
    assert!(err.contains("cannot convert this YAML node to TOML"));
}

#[test]
fn to_toml_includes_fix_policy() {
    let td = tempdir().unwrap();
    let cfg_path = td.path().join(".ryl.toml");
    fs::write(
        &cfg_path,
        "[fix]\nfixable = ['ALL', 'comments', 'new-lines']\nunfixable = ['new-line-at-end-of-file']\n",
    )
    .unwrap();

    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg_path),
            config_data: None,
        },
    )
    .unwrap();

    let toml = ctx.config.to_toml_string().unwrap();
    assert!(toml.contains("[fix]"));
    assert!(toml.contains("fixable = ["));
    assert!(toml.contains("\"ALL\""));
    assert!(toml.contains("\"comments\""));
    assert!(toml.contains("\"new-lines\""));
    assert!(toml.contains("unfixable = ["));
    assert!(toml.contains("\"new-line-at-end-of-file\""));
}
