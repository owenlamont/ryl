use ryl::config::{Overrides, discover_config};

#[test]
fn rules_with_non_string_key_are_skipped() {
    // Include a non-string key in the rules map to exercise the `continue` path.
    // YAML: a sequence used as a key plus a normal string key.
    let cfg = r#"
rules:
  ? [1, 2]
  : { level: warning }
  anchors: { forbid-undeclared-aliases: false }
"#;
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("parse");
    // The valid rule should still be present; the non-string key entry is ignored.
    assert!(ctx.config.rule_names().iter().any(|r| r == "anchors"));
}
