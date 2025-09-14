use ryl::config::{Overrides, discover_config};

#[test]
fn merge_from_covers_existing_and_new_rule_paths() {
    let yaml = r#"
extends: default
rules:
  comments:
    level: error
  brand_new_rule:
    enabled: true
"#;
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect("parse");
    let names = ctx.config.rule_names();
    assert!(names.iter().any(|n| n == "comments"));
    assert!(names.iter().any(|n| n == "brand_new_rule"));
}
