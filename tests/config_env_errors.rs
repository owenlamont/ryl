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

#[test]
fn env_toml_target_is_rejected_even_when_missing() {
    // The `.toml` rejection keys on the extension (the loader's sole YAML-vs-TOML signal)
    // and fires before the existence check, so pointing yamllint's env var at ryl-native
    // TOML is flagged regardless of whether the file is present (unlike a missing YAML
    // target, which is silently ignored).
    let td = tempdir().unwrap();
    let missing = td.path().join("ryl.toml");
    let inputs = vec![td.path().join("input.yaml")];
    let err = discover_config_with_env(&inputs, &Overrides::default(), &|k| match k {
        "YAMLLINT_CONFIG_FILE" => Some(missing.display().to_string()),
        // HOME bounds the project-config walk to the tempdir.
        "HOME" => Some(td.path().display().to_string()),
        _ => None,
    })
    .unwrap_err();
    assert!(
        err.contains("only yamllint YAML"),
        "expected TOML rejection, got: {err}"
    );
}
