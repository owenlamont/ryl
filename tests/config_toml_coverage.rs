use std::fs;

use ryl::config::{Overrides, discover_config, discover_per_file};
use tempfile::tempdir;

#[test]
fn discover_config_uses_project_toml_and_parses_float_values() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join(".ryl.toml"),
        "flag = true\nratio = 1.25\nstamp = 1979-05-27T07:32:00Z\n[rules]\nanchors = 'disable'\n",
    )
    .unwrap();
    let file = root.join("file.yaml");
    fs::write(&file, "a: 1\n").unwrap();

    let ctx = discover_config(&[file], &Overrides::default()).expect("project TOML");
    assert_eq!(
        ctx.source.as_deref(),
        Some(root.join(".ryl.toml").as_path())
    );
}

#[test]
fn discover_config_skips_pyproject_without_tool_ryl_in_project_search() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = 'demo'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fs::write(root.join(".yamllint"), "locale: en_US.UTF-8\nrules: {}\n").unwrap();
    let file = root.join("file.yaml");
    fs::write(&file, "a: 1\n").unwrap();

    let ctx = discover_config(&[file], &Overrides::default()).expect("yaml fallback");
    assert_eq!(
        ctx.source.as_deref(),
        Some(root.join(".yamllint").as_path())
    );
}

#[test]
fn discover_per_file_uses_project_toml() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join(".ryl.toml"), "[rules]\nanchors = 'disable'\n").unwrap();
    let file = root.join("file.yaml");
    fs::write(&file, "a: 1\n").unwrap();

    let ctx = discover_per_file(&file).expect("per-file project TOML");
    assert_eq!(
        ctx.source.as_deref(),
        Some(root.join(".ryl.toml").as_path())
    );
}

#[test]
fn explicit_pyproject_config_file_with_tool_ryl_loads() {
    let td = tempdir().unwrap();
    let root = td.path();
    let pyproject = root.join("pyproject.toml");
    fs::write(
        &pyproject,
        "[project]\nname = 'demo'\nversion = '0.1.0'\n[tool.ryl]\nlocale = 'it_IT.UTF-8'\n",
    )
    .unwrap();

    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(pyproject),
            config_data: None,
        },
    )
    .expect("explicit pyproject [tool.ryl]");
    assert_eq!(ctx.config.locale(), Some("it_IT.UTF-8"));
}

#[test]
fn per_file_ignores_reject_invalid_pattern() {
    let td = tempdir().unwrap();
    let cfg = td.path().join(".ryl.toml");
    fs::write(
        &cfg,
        "[rules]\ndocument-start = 'enable'\n[per-file-ignores]\n'[' = ['document-start']\n",
    )
    .unwrap();

    let err = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
    )
    .unwrap_err();
    assert!(err.contains("per-file-ignores pattern '[' is invalid"));
}

#[test]
fn per_file_ignores_reject_invalid_absolute_pattern() {
    let td = tempdir().unwrap();
    let root = td.path().join("[root");
    fs::create_dir(&root).unwrap();
    let cfg = root.join(".ryl.toml");
    fs::write(
        &cfg,
        "[rules]\ndocument-start = 'enable'\n[per-file-ignores]\n'file.yaml' = ['document-start']\n",
    )
    .unwrap();

    let err = discover_config(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
    )
    .unwrap_err();
    assert!(err.contains("per-file-ignores pattern 'file.yaml' is invalid"));
}
