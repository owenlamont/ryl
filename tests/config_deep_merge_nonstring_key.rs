use ryl::config::{Overrides, discover_config};

#[test]
fn deep_merge_skips_non_string_inner_keys() {
    // default has `comments` as a mapping; we override with a mapping that contains
    // a non-string key to trigger `continue` inside deep_merge_yaml_owned.
    let cfg = r#"
extends: default
rules:
  comments:
    ? [1, 2]
    : 9
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
    assert!(ctx.config.rule_names().iter().any(|r| r == "comments"));
}
