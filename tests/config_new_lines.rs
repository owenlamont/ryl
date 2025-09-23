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
    assert_eq!(
        err,
        "invalid config: unknown option \"foo\" for rule \"new-lines\""
    );
}

#[test]
fn invalid_type_value_errors() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    type: invalid\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"type\" of \"new-lines\" should be in ('unix', 'dos', 'platform')"
    );
}

#[test]
fn invalid_type_kind_errors_on_non_string() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    type: [unix]\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"type\" of \"new-lines\" should be in ('unix', 'dos', 'platform')"
    );
}

#[test]
fn unknown_option_reports_numeric_key() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    1: value\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1\" for rule \"new-lines\""
    );
}

#[test]
fn unknown_option_reports_boolean_key() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    true: value\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"true\" for rule \"new-lines\""
    );
}

#[test]
fn unknown_option_reports_float_key() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    1.5: value\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"1.5\" for rule \"new-lines\""
    );
}

#[test]
fn unknown_option_reports_null_key() {
    let err = discover_with_yaml("rules:\n  new-lines:\n    null: value\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"None\" for rule \"new-lines\""
    );
}
