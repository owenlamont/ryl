use ryl::config::{Overrides, discover_config};
use std::fs;
use tempfile::tempdir;

#[test]
fn ignore_block_supports_negation() {
    let td = tempdir().unwrap();
    let cfg = td.path().join("conf.yaml");
    fs::write(
        &cfg,
        "ignore: |\n  **/*.skip.yaml\n  !**/keep.skip.yaml\n  nested/\nyaml-files: []\nrules: {}\n",
    )
    .unwrap();

    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg.clone()),
            config_data: None,
        },
    )
    .expect("config parse");
    let base = ctx.base_dir.clone();

    assert!(
        ctx.config
            .is_file_ignored(&td.path().join("a.skip.yaml"), &base)
    );
    assert!(
        !ctx.config
            .is_file_ignored(&td.path().join("keep.skip.yaml"), &base)
    );
    assert!(
        ctx.config
            .is_file_ignored(&td.path().join("nested/file.yaml"), &base)
    );
}

#[test]
fn ignore_from_file_patterns_are_loaded() {
    let td = tempdir().unwrap();
    let cfg = td.path().join("conf.yaml");
    let ignore = td.path().join(".yamllint-ignore");
    fs::write(
        &cfg,
        "ignore-from-file: .yamllint-ignore\nyaml-files: []\nrules: {}\n",
    )
    .unwrap();
    fs::write(&ignore, "*.gen.yaml\n!.keep.yaml\n").unwrap();

    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg.clone()),
            config_data: None,
        },
    )
    .expect("config parse");
    let base = ctx.base_dir.clone();

    assert!(
        ctx.config
            .is_file_ignored(&td.path().join("output.gen.yaml"), &base)
    );
    assert!(
        !ctx.config
            .is_file_ignored(&td.path().join(".keep.yaml"), &base)
    );
    assert!(
        ctx.config
            .ignore_patterns()
            .iter()
            .any(|p| p == "*.gen.yaml")
    );
}

#[test]
fn extends_resolves_relative_file_paths() {
    let td = tempdir().unwrap();
    let base_cfg = td.path().join("base.yaml");
    fs::write(
        &base_cfg,
        "ignore: ['base/**']\nyaml-files: []\nrules: {}\n",
    )
    .unwrap();
    let child_cfg = td.path().join("child.yaml");
    fs::write(
        &child_cfg,
        "extends: base.yaml\nignore: ['child/**']\nyaml-files: []\nrules: {}\n",
    )
    .unwrap();

    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(child_cfg.clone()),
            config_data: None,
        },
    )
    .expect("config parse");
    let base = ctx.base_dir.clone();

    assert!(
        ctx.config
            .is_file_ignored(&td.path().join("base/one.yaml"), &base)
    );
    assert!(
        ctx.config
            .is_file_ignored(&td.path().join("child/two.yaml"), &base)
    );
}

#[test]
fn config_data_builtin_name_expands() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("extends: relaxed".to_string()),
        },
    )
    .expect("config parse");

    assert!(ctx.config.rule_names().iter().any(|r| r == "braces"));
}
