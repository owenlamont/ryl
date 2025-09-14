use ryl::config::{Overrides, discover_config};

#[test]
fn from_yaml_str_covers_ignore_yamlfiles_and_rules_paths() {
    let yaml = r#"
ignore: ["a.yml", 1, "b.yaml"]
yaml-files: "*.yml"
rules:
  new_rule: { enabled: true }
"#;
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect("config parse");

    let pats = ctx.config.ignore_patterns();
    assert!(pats.iter().any(|p| p == "a.yml"));
    assert!(pats.iter().any(|p| p == "b.yaml"));

    assert!(ctx.config.rule_names().iter().any(|n| n == "new_rule"));
}
