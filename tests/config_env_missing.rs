use std::fs;

use ryl::config::{Env, Overrides, SystemEnv, discover_config_with_env};
use tempfile::tempdir;

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

#[test]
fn env_tilde_path_uses_closure_home_dir() {
    let dir = tempdir().unwrap();
    let config_path = dir
        .path()
        .join(".config")
        .join("yamllint")
        .join("custom.yml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    fs::write(&config_path, "rules: {}\n").unwrap();

    let inputs: Vec<std::path::PathBuf> = vec![];
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), &|k| match k {
        "YAMLLINT_CONFIG_FILE" => Some("~/.config/yamllint/custom.yml".into()),
        "HOME" => Some(dir.path().to_str().unwrap().into()),
        _ => None,
    })
    .expect("discover should succeed");

    assert_eq!(ctx.source.as_deref(), Some(config_path.as_path()));
}

#[test]
fn env_tilde_path_without_home_falls_back_to_system_home() {
    let inputs: Vec<std::path::PathBuf> = vec![];
    let _ = SystemEnv.home_dir();
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), &|k| {
        if k == "YAMLLINT_CONFIG_FILE" {
            Some("~/.config/yamllint/missing.yml".into())
        } else {
            None
        }
    })
    .expect("discover should succeed");

    // No config file found; default preset applies.
    assert!(ctx.source.is_none());
}
