use std::path::{Path, PathBuf};

use ryl::config::{Overrides, discover_config_with};

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

#[test]
fn project_search_respects_home_boundary() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/workspace"))
        .with_var("HOME", "/workspace/userhome".to_string())
        .with_exists(PathBuf::from("/workspace/.yamllint"))
        .with_file(
            PathBuf::from("/workspace/.yamllint"),
            "rules: { custom: enable }\n",
        );

    let ctx = discover_config_with(
        &[PathBuf::from("/workspace/userhome/project/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("config discovery should succeed");
    assert!(ctx.source.is_none(), "project config should not cross HOME");
    assert!(ctx.config.rule_names().iter().any(|r| r == "anchors"));
    assert!(!ctx.config.rule_names().iter().any(|r| r == "custom"));
}

#[test]
fn project_search_supports_relative_home() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/workspace"))
        .with_var("HOME", "userhome".to_string())
        .with_file(
            PathBuf::from("/workspace/userhome/.yamllint"),
            "rules: { rel: enable }\n",
        )
        .with_exists(PathBuf::from("/workspace/userhome/.yamllint"))
        .with_exists(PathBuf::from("/workspace/userhome/project/file.yaml"));

    let ctx = discover_config_with(
        &[PathBuf::from("/workspace/userhome/project/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("relative HOME should be joined with cwd");
    assert_eq!(
        ctx.source.as_deref(),
        Some(Path::new("/workspace/userhome/.yamllint"))
    );
    assert!(ctx.config.rule_names().iter().any(|r| r == "rel"));
}
