use std::path::PathBuf;

use ryl::config::{Overrides, discover_config, discover_config_with};

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

#[test]
fn locale_value_is_parsed() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("locale: en_US.UTF-8\nrules: {}\n".into()),
        },
    )
    .expect("locale should parse");
    assert_eq!(ctx.config.locale(), Some("en_US.UTF-8"));
}

#[test]
fn locale_from_child_overrides_base() {
    let root = PathBuf::from("/workspace");
    let child = root.join("child.yml");
    let base = root.join("base.yml");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(child.clone(), "locale: fr_FR.UTF-8\nextends: base.yml\n")
        .with_exists(child.clone())
        .with_file(base.clone(), "locale: en_US.UTF-8\n")
        .with_exists(base.clone());

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(child),
            config_data: None,
        },
        &env,
    )
    .expect("child locale should win");
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
}

#[test]
fn locale_falls_back_to_base_when_missing() {
    let root = PathBuf::from("/workspace");
    let child = root.join("child.yml");
    let base = root.join("base.yml");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(child.clone(), "extends: base.yml\n")
        .with_exists(child.clone())
        .with_file(base.clone(), "locale: en_US.UTF-8\n")
        .with_exists(base.clone());

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(child),
            config_data: None,
        },
        &env,
    )
    .expect("locale should bubble from base");
    assert_eq!(ctx.config.locale(), Some("en_US.UTF-8"));
}

#[test]
fn locale_non_string_errors() {
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("locale: [1]\n".into()),
        },
    )
    .expect_err("non-string locale should error");
    assert!(err.contains("locale should be a string"));
}
