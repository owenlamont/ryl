use ryl::config::{Overrides, discover_config};

#[test]
fn rules_with_non_string_key_are_rejected() {
    let cfg = r#"
rules:
  ? [1, 2]
  : { level: warning }
  anchors: { forbid-undeclared-aliases: false }
"#;
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("non-string rule names should fail typed YAML parsing");
    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
