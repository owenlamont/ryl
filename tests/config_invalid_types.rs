use ryl::config::{Overrides, discover_config};

#[test]
fn invalid_types_for_ignore_and_yaml_files_error() {
    let yaml = r#"
ignore: { bad: 1 }
yaml-files: { bad: 2 }
rules: {}
"#;
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .unwrap_err();
    assert!(err.contains("invalid config"));
}
