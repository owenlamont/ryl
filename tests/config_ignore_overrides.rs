use std::path::PathBuf;

use ryl::config::{Overrides, discover_config_with};

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

#[test]
fn child_ignore_patterns_replace_parent_entries() {
    let root = PathBuf::from("/workspace");
    let base_cfg = root.join("base.yml");
    let child_cfg = root.join("child.yml");

    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(base_cfg.clone(), "rules: {}\nignore: ['parent.yaml']\n")
        .with_file(
            child_cfg.clone(),
            "extends: base.yml\nignore: ['child.yaml']\nrules: {}\n",
        );

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(child_cfg),
            config_data: None,
        },
        &env,
    )
    .expect("child config should parse");

    let base_dir = ctx.base_dir.clone();
    assert!(
        ctx.config
            .is_file_ignored(&base_dir.join("child.yaml"), &base_dir),
        "child.yaml should be ignored by child config"
    );
    assert!(
        !ctx.config
            .is_file_ignored(&base_dir.join("parent.yaml"), &base_dir),
        "parent.yaml should not use parent ignore entries"
    );
}

#[test]
fn child_ignore_from_file_replaces_parent_patterns() {
    let root = PathBuf::from("/workspace");
    let base_cfg = root.join("base.yml");
    let child_cfg = root.join("child.yml");
    let ignore_file = root.join("child.ignore");

    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(base_cfg.clone(), "rules: {}\nignore: ['parent.yaml']\n")
        .with_file(
            child_cfg.clone(),
            "extends: base.yml\nignore-from-file: child.ignore\nrules: {}\n",
        )
        .with_file(ignore_file.clone(), "child.yaml\n");

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(child_cfg),
            config_data: None,
        },
        &env,
    )
    .expect("child config should parse");

    let base_dir = ctx.base_dir.clone();
    assert!(
        ctx.config
            .is_file_ignored(&base_dir.join("child.yaml"), &base_dir),
        "child.yaml should respect child ignore-from-file entries"
    );
    assert!(
        !ctx.config
            .is_file_ignored(&base_dir.join("parent.yaml"), &base_dir),
        "parent ignores should be replaced by child ignore-from-file"
    );
}
