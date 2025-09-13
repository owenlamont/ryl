use ryl::config::{Overrides, discover_config_with_env};

#[test]
fn env_points_to_missing_file_is_ignored() {
    let inputs: Vec<std::path::PathBuf> = vec![];
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), |k| {
        if k == "YAMLLINT_CONFIG_FILE" {
            Some("/tmp/this/does/not/exist.yml".into())
        } else {
            None
        }
    })
    .expect("discover should succeed");
    // Fallback to empty config
    assert!(ctx.config.rule_names().is_empty());
}
