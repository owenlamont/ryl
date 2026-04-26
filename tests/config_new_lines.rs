use ryl::config::{Overrides, discover_config};

fn discover_with_yaml(yaml: &str) -> Result<(), String> {
    discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.to_string()),
        },
    )
    .map(|_| ())
}

#[test]
fn unknown_option_errors() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    foo: bar\n").unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.new-lines"), "{err}");
}

#[test]
fn invalid_type_value_errors() {
    let err =
        discover_with_yaml("rules:\n  new-lines:\n    type: invalid\n").unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.new-lines"), "{err}");
}

#[test]
fn invalid_type_kind_errors_on_non_string() {
    let err =
        discover_with_yaml("rules:\n  new-lines:\n    type: [unix]\n").unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.new-lines"), "{err}");
}

#[test]
fn unknown_option_reports_numeric_key() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    1: value\n").unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}

#[test]
fn unknown_option_reports_boolean_key() {
    let err =
        discover_with_yaml("rules:\n  new-lines:\n    true: value\n").unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}

#[test]
fn unknown_option_reports_float_key() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    1.5: value\n").unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}

#[test]
fn unknown_option_reports_tagged_key() {
    let err =
        discover_with_yaml("rules:\n  new-lines:\n    !foo bar: value\n").unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}

#[test]
fn unknown_option_reports_null_key() {
    let err =
        discover_with_yaml("rules:\n  new-lines:\n    null: value\n").unwrap_err();
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
