use std::path::PathBuf;

use ryl::config::{Overrides, discover_config_with};

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

#[test]
fn discover_returns_none_when_no_configs_present() {
    let env = FakeEnv::new().with_cwd(PathBuf::from("/proj"));
    let inputs = vec![PathBuf::from("/proj")];
    let ctx = discover_config_with(&inputs, &Overrides::default(), &env)
        .expect("discovery without configs should succeed");
    assert!(ctx.source.is_none());
}

#[test]
fn discover_uses_env_config_when_set() {
    let cfg_path = PathBuf::from("/cfg/envcfg.yml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/proj"))
        .with_file(cfg_path.clone(), "ignore: ['**/skip/**']\n")
        .with_var("YAMLLINT_CONFIG_FILE", "/cfg/envcfg.yml");
    let inputs = vec![PathBuf::from("/proj")];
    let ctx = discover_config_with(&inputs, &Overrides::default(), &env)
        .expect("env-pointed config should load");
    assert_eq!(ctx.source.as_deref(), Some(cfg_path.as_path()));
}

#[test]
fn discover_errors_when_project_config_is_unreadable() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/proj"))
        .with_exists(PathBuf::from("/proj/.yamllint"));
    let inputs = vec![PathBuf::from("/proj/file.yaml")];
    let err = discover_config_with(&inputs, &Overrides::default(), &env)
        .expect_err("unreadable project config should error");
    assert!(err.contains("failed to read"), "unexpected error: {err}");
}

#[test]
fn discover_uses_user_global_when_no_project_config() {
    // The yamllint-compat user-global resolves via XDG_CONFIG_HOME, matching yamllint.
    let global_cfg = PathBuf::from("/xdg/yamllint/config");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/proj"))
        .with_var("XDG_CONFIG_HOME", "/xdg")
        .with_file(global_cfg.clone(), "ignore: ['**/a.yaml']\n");
    let inputs = vec![PathBuf::from("/proj")];
    let ctx = discover_config_with(&inputs, &Overrides::default(), &env)
        .expect("user-global config should load");
    assert_eq!(ctx.source.as_deref(), Some(global_cfg.as_path()));
}

#[test]
fn discover_errors_on_env_config_parse_error() {
    let cfg_path = PathBuf::from("/cfg/envcfg.yml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/proj"))
        .with_file(cfg_path, "rules: {\n")
        .with_var("YAMLLINT_CONFIG_FILE", "/cfg/envcfg.yml");
    let inputs = vec![PathBuf::from("/proj")];
    let err = discover_config_with(&inputs, &Overrides::default(), &env)
        .expect_err("malformed env-pointed config should fail");
    assert!(
        err.contains("failed to parse config data"),
        "unexpected error: {err}"
    );
}

#[test]
fn discover_errors_on_project_config_parse_error() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/proj"))
        .with_file(PathBuf::from("/proj/.yamllint"), "rules: {\n");
    let inputs = vec![PathBuf::from("/proj/file.yaml")];
    let err = discover_config_with(&inputs, &Overrides::default(), &env)
        .expect_err("malformed project config should fail");
    assert!(
        err.contains("failed to parse config data"),
        "unexpected error: {err}"
    );
}

#[test]
fn user_config_migration_paths_resolves_source_and_target() {
    let env = FakeEnv::new()
        .with_var("XDG_CONFIG_HOME", "/xdg")
        .with_config_dir(PathBuf::from("/xdg"));
    let (source, target) =
        ryl::config::user_config_migration_paths(&env).expect("both paths resolve");
    assert_eq!(source, PathBuf::from("/xdg/yamllint/config"));
    assert_eq!(target, PathBuf::from("/xdg/ryl/ryl.toml"));
}

#[test]
fn user_config_migration_paths_none_without_home_or_xdg() {
    // No XDG_CONFIG_HOME and no home: the yamllint source is unresolvable.
    let env = FakeEnv::new();
    assert!(ryl::config::user_config_migration_paths(&env).is_none());
}

#[test]
fn user_config_migration_paths_none_without_config_dir() {
    // Source resolves via home, but no config dir means no ryl target.
    let env = FakeEnv::new().with_home(PathBuf::from("/home/u"));
    assert!(ryl::config::user_config_migration_paths(&env).is_none());
}

#[test]
fn discover_errors_when_user_global_config_is_unreadable() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/proj"))
        .with_var("XDG_CONFIG_HOME", "/xdg")
        .with_exists(PathBuf::from("/xdg/yamllint/config"));
    let inputs = vec![PathBuf::from("/proj")];
    let err = discover_config_with(&inputs, &Overrides::default(), &env)
        .expect_err("unreadable user-global config should fail");
    assert!(err.contains("failed to read"), "unexpected error: {err}");
}
