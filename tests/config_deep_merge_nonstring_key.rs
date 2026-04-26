use ryl::config::{Overrides, discover_config};

#[test]
fn deep_merge_rejects_non_string_inner_keys() {
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
    );
    let err = ctx.expect_err("non-string keys should now fail during typed parse");
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
