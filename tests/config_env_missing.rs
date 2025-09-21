use ryl::config::{Overrides, discover_config_with_env};

#[test]
fn env_points_to_missing_file_is_ignored() {
    let inputs: Vec<std::path::PathBuf> = vec![];
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), &|k| {
        if k == "YAMLLINT_CONFIG_FILE" {
            Some("/tmp/this/does/not/exist.yml".into())
        } else {
            None
        }
    })
    .expect("discover should succeed");
    // Missing env config falls back to default preset
    assert!(ctx.source.is_none());
    assert!(ctx.config.rule_names().iter().any(|r| r == "anchors"));
}
