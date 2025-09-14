use std::fs;
use std::path::PathBuf;

use ryl::config::{Overrides, discover_config_with_env};
use tempfile::tempdir;

#[test]
fn discover_config_with_env_respects_inline_data() {
    let inline = "ignore: ['a.yaml']\n".to_string();
    let inputs: Vec<PathBuf> = vec![];
    let ctx = discover_config_with_env(
        &inputs,
        &Overrides {
            config_file: None,
            config_data: Some(inline),
        },
        |_k| None,
    )
    .expect("inline data parsed");
    assert!(ctx.config.ignore_patterns().iter().any(|p| p == "a.yaml"));
}

#[test]
fn discover_config_with_env_respects_explicit_file() {
    let td = tempdir().unwrap();
    let file = td.path().join("cfg.yml");
    fs::write(&file, "ignore: ['b.yaml']\n").unwrap();
    let inputs: Vec<PathBuf> = vec![];
    let ctx = discover_config_with_env(
        &inputs,
        &Overrides {
            config_file: Some(file.clone()),
            config_data: None,
        },
        |_k| None,
    )
    .expect("file parsed");
    assert!(ctx.config.ignore_patterns().iter().any(|p| p == "b.yaml"));
}
