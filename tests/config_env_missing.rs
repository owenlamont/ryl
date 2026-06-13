use std::fs;

use ryl::config::{Env, Overrides, SystemEnv, discover_config_with_env};
use tempfile::tempdir;

#[test]
fn env_points_to_missing_file_is_ignored() {
    let workspace = tempdir().unwrap();
    let inputs = vec![workspace.path().join("input.yaml")];
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), &|k| {
        if k == "YAMLLINT_CONFIG_FILE" {
            Some("/tmp/this/does/not/exist.yml".into())
        } else {
            None
        }
    })
    .expect("discover should succeed");
    // A missing env config is ignored; with nothing else found, resolution falls back
    // to an empty config (explicit-enable model), not the default preset.
    assert!(ctx.source.is_none());
    assert!(!ctx.config_found);
    assert!(ctx.config.rule_names().is_empty());
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

    let project_root = dir.path().join("workspace");
    fs::create_dir_all(&project_root).unwrap();
    let inputs = vec![project_root.join("input.yaml")];
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), &|k| match k {
        "YAMLLINT_CONFIG_FILE" => Some("~/.config/yamllint/custom.yml".into()),
        "HOME" => Some(dir.path().to_str().unwrap().into()),
        _ => None,
    })
    .expect("discover should succeed");

    assert_eq!(ctx.source.as_deref(), Some(config_path.as_path()));
}

#[test]
fn env_xdg_config_home_discovers_ryl_native_user_global() {
    // discover_config_with_env routes XDG_CONFIG_HOME through the closure, so the
    // ryl-native user-global (<config-dir>/ryl/ryl.toml) is discoverable via that API.
    let dir = tempdir().unwrap();
    let ryl_cfg = dir.path().join("ryl").join("ryl.toml");
    fs::create_dir_all(ryl_cfg.parent().unwrap()).unwrap();
    fs::write(&ryl_cfg, "[rules]\nkey-duplicates = 'enable'\n").unwrap();

    let project_root = dir.path().join("workspace");
    fs::create_dir_all(&project_root).unwrap();
    let inputs = vec![project_root.join("input.yaml")];
    // HOME bounds the project-config walk to the tempdir; XDG_CONFIG_HOME points the
    // user-global lookup at the tempdir so the test never reads the real config dir.
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), &|k| match k {
        "XDG_CONFIG_HOME" => Some(dir.path().to_str().unwrap().into()),
        "HOME" => Some(dir.path().to_str().unwrap().into()),
        _ => None,
    })
    .expect("discover should succeed");

    assert_eq!(ctx.source.as_deref(), Some(ryl_cfg.as_path()));
}

#[test]
fn env_tilde_path_without_home_falls_back_to_system_home() {
    let workspace = tempdir().unwrap();
    let inputs = vec![workspace.path().join("input.yaml")];
    let _ = SystemEnv.home_dir();
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), &|k| {
        if k == "YAMLLINT_CONFIG_FILE" {
            Some("~/.config/yamllint/missing.yml".into())
        } else {
            None
        }
    })
    .expect("discover should succeed");

    // No config file found; resolution falls back to an empty config.
    assert!(ctx.source.is_none());
    assert!(!ctx.config_found);
}
