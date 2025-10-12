use std::fs;

use ryl::config::{Overrides, discover_config_with_env};
use tempfile::tempdir;

#[test]
fn env_points_to_unreadable_path_errors() {
    let td = tempdir().unwrap();
    let dir = td.path().join("cfgdir");
    fs::create_dir_all(&dir).unwrap();
    let inputs = vec![dir.join("input.yaml")];
    let res = discover_config_with_env(&inputs, &Overrides::default(), &|k| {
        if k == "YAMLLINT_CONFIG_FILE" {
            Some(dir.display().to_string())
        } else {
            None
        }
    });
    assert!(
        res.is_err(),
        "expected error when env points to a directory, got {res:?}"
    );
}
