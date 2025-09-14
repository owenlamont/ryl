use ryl::config::{Overrides, discover_config};

#[test]
fn rules_merge_deep_and_replace_scalars() {
    // Start from default (anchors: enable [scalar], comments: { level: warning } [map])
    // Then replace anchors with a mapping and extend comments with an extra key.
    let cfg = r#"
extends: default
rules:
  anchors:
    style: custom
  comments:
    extra: 1
    level: error
"#;
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("parse");
    assert!(ctx.config.rule_names().contains(&"anchors".to_string()));
    assert!(ctx.config.rule_names().contains(&"comments".to_string()));
}
