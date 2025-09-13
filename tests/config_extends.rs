use ryl::config::{Overrides, discover_config};

#[test]
fn extends_empty_and_custom_rules_and_ignores_merge() {
    let cfg = r#"
extends: empty
rules:
  custom-rule: {}
ignore: ['docs/**']
"#;

    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("config parse");
    assert!(ctx.config.rule_names().iter().any(|s| s == "custom-rule"));
    assert!(ctx.config.ignore_patterns().iter().any(|s| s == "docs/**"));
}

#[test]
fn extends_default_adds_some_rules() {
    let cfg = "extends: default\n";
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("config parse");
    assert!(!ctx.config.rule_names().is_empty());
}
