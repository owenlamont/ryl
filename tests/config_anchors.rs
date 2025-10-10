use ryl::config::{Overrides, discover_config};

#[test]
fn anchors_allows_boolean_options() {
    let cfg = r#"
rules:
  anchors:
    forbid-undeclared-aliases: false
    forbid-duplicated-anchors: true
    forbid-unused-anchors: true
"#;

    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("parse");

    assert!(ctx.config.rule_names().iter().any(|r| r == "anchors"));
}

#[test]
fn anchors_rejects_non_bool_option() {
    let cfg = r#"
rules:
  anchors:
    forbid-duplicated-anchors: "yes"
"#;

    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("invalid value");

    assert!(err.contains("forbid-duplicated-anchors"));
}

#[test]
fn anchors_rejects_unknown_option() {
    let cfg = r#"
rules:
  anchors:
    unknown: true
"#;

    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("unknown option");

    assert!(err.contains("unknown option \"unknown\" for rule \"anchors\""));
}

#[test]
fn anchors_rejects_non_string_option_key() {
    let cfg = r#"
rules:
  anchors:
    ? [1, 2]
    : true
"#;

    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("non-string key");

    assert!(err.contains("unknown option") && err.contains("anchors"));
}
