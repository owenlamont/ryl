use ryl::config::{Overrides, discover_config};

#[test]
fn yaml_files_sequence_with_non_string_items_is_skipped() {
    let yaml = r#"
yaml-files: ["*.yml", 1]
rules: {}
"#;
    let _ = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect("ok");
}
