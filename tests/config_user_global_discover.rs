use std::fs;
use std::path::PathBuf;

use ryl::config::{Overrides, discover_config};
use tempfile::tempdir;

#[test]
fn discover_config_uses_user_global_when_no_project_or_overrides() {
    let td = tempdir().unwrap();

    // Create an XDG-style user-global config: $XDG_CONFIG_HOME/yamllint/config
    let xdg = td.path().join("xdg").join("yamllint");
    fs::create_dir_all(&xdg).unwrap();
    let global_cfg = xdg.join("config");
    fs::write(&global_cfg, "ignore: ['**/a.yaml']\n").unwrap();

    // Project tree without any project config
    let proj = td.path().join("proj");
    fs::create_dir_all(&proj).unwrap();

    // Ensure the library resolves user-global config for discovery
    // Safety: setting a process env var for test isolation only.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", td.path().join("xdg"));
    }
    let inputs: Vec<PathBuf> = vec![proj.clone()];
    let ctx = discover_config(&inputs, &Overrides::default()).expect("discover user-global");

    // Validate it came from our global config and behaves as expected
    assert_eq!(ctx.source.as_deref(), Some(global_cfg.as_path()));
    let a = proj.join("a.yaml");
    assert!(ctx.config.is_file_ignored(&a, &ctx.base_dir));
}
