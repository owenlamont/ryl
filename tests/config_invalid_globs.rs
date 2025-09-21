use ryl::config::{Overrides, discover_config};

#[test]
fn invalid_ignore_and_yaml_file_patterns_error() {
    // '[' is an invalid glob; configuration should fail to parse.
    let cfg = "ignore: ['[']\nyaml-files: ['[']\n";
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .unwrap_err();
    assert!(err.contains("invalid config"));
}
