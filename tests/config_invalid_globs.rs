use ryl::config::{Overrides, discover_config};

#[test]
fn invalid_ignore_and_yaml_file_patterns_are_ignored() {
    // '[' is an invalid glob; both ignore and yaml-files entries should be skipped safely.
    let cfg = "ignore: ['[']\nyaml-files: ['[']\n";
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("parse config");
    // Invalid ignore pattern should not ignore files.
    assert!(
        !ctx.config
            .is_file_ignored(&std::path::PathBuf::from("a.yaml"), &ctx.base_dir)
    );
}
