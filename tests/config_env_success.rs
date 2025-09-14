use std::fs;

use ryl::config::{Overrides, discover_config_with_env};
use tempfile::tempdir;

#[test]
fn env_points_to_valid_config_applies() {
    let td = tempdir().unwrap();
    let cfg = td.path().join("envcfg.yml");
    fs::write(&cfg, "ignore: ['**/skipme/**']\n").unwrap();

    let inputs: Vec<std::path::PathBuf> = vec![];
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), &|k| {
        if k == "YAMLLINT_CONFIG_FILE" {
            Some(cfg.display().to_string())
        } else {
            None
        }
    })
    .expect("env config should load");

    let base = ctx.base_dir.clone();
    let path = base.join("a/skipme/file.yaml");
    assert!(ctx.config.is_file_ignored(&path, &ctx.base_dir));
}
