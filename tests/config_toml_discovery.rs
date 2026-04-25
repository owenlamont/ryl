use std::path::{Path, PathBuf};

use ryl::config::{Overrides, discover_config_with, discover_per_file};
use tempfile::tempdir;

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

#[test]
fn project_toml_takes_precedence_over_yaml_and_adds_notice() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/.ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_file(
            PathBuf::from("/repo/.yamllint"),
            "locale: en_US.UTF-8\nrules: {}\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("project TOML should load");
    assert_eq!(ctx.source.as_deref(), Some(Path::new("/repo/.ryl.toml")));
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
    assert_eq!(ctx.notices.len(), 1);
    assert!(ctx.notices[0].contains("ignoring legacy YAML config discovery"));
}

#[test]
fn explicit_pyproject_with_tool_ryl_section_loads() {
    let pyproject = PathBuf::from("/repo/pyproject.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        pyproject.clone(),
        "[project]\nname = 'demo'\nversion = '0.1.0'\n[tool.ryl]\nlocale = 'en_GB.UTF-8'\n",
    );
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(pyproject),
            config_data: None,
        },
        &env,
    )
    .expect("explicit pyproject [tool.ryl] should load");
    assert_eq!(ctx.config.locale(), Some("en_GB.UTF-8"));
}

#[test]
fn project_pyproject_without_tool_ryl_falls_back_to_yaml() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/pyproject.toml"),
            "[project]\nname = 'demo'\nversion = '0.1.0'\n",
        )
        .with_file(
            PathBuf::from("/repo/.yamllint"),
            "locale: en_US.UTF-8\nrules: {}\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("yaml fallback should load");
    assert_eq!(ctx.source.as_deref(), Some(Path::new("/repo/.yamllint")));
    assert_eq!(ctx.config.locale(), Some("en_US.UTF-8"));
    assert!(ctx.notices.is_empty());
}

#[test]
fn discover_per_file_finds_project_toml() {
    let td = tempdir().unwrap();
    let root = td.path();
    std::fs::write(root.join(".ryl.toml"), "[rules]\nanchors = 'disable'\n").unwrap();
    std::fs::write(root.join("file.yaml"), "a: 1\n").unwrap();
    let ctx = discover_per_file(&root.join("file.yaml")).expect("per-file discovery");
    assert_eq!(
        ctx.source.as_deref(),
        Some(root.join(".ryl.toml").as_path())
    );
}

#[test]
fn project_skips_non_ryl_pyproject_and_uses_parent_toml() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/sub/pyproject.toml"),
            "[project]\nname = 'sub'\nversion = '0.1.0'\n",
        )
        .with_file(
            PathBuf::from("/repo/.ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_exists(PathBuf::from("/repo/sub/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/sub/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("ancestor TOML should load");
    assert_eq!(ctx.source.as_deref(), Some(Path::new("/repo/.ryl.toml")));
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
}

#[test]
fn project_pyproject_with_tool_ryl_is_used() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/pyproject.toml"),
            "[project]\nname = 'demo'\nversion = '0.1.0'\n[tool.ryl]\nlocale = 'de_DE.UTF-8'\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("pyproject [tool.ryl] should load");
    assert_eq!(
        ctx.source.as_deref(),
        Some(Path::new("/repo/pyproject.toml"))
    );
    assert_eq!(ctx.config.locale(), Some("de_DE.UTF-8"));
}

#[test]
fn explicit_pyproject_requires_tool_ryl_section() {
    let pyproject = PathBuf::from("/repo/pyproject.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        pyproject.clone(),
        "[project]\nname = 'demo'\nversion = '0.1.0'\n",
    );
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(pyproject),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("missing [tool.ryl] section"));
}

#[test]
fn toml_config_rejects_extends_key() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "extends = 'relaxed'\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("extends is not supported in TOML configuration"));
}

#[test]
fn explicit_toml_parse_error_is_reported() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "rules = [\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn toml_scalar_types_are_accepted_for_unknown_keys() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        cfg.clone(),
        "flag = true\nratio = 1.5\nstamp = 1979-05-27T07:32:00Z\n[rules]\nanchors = 'disable'\n",
    );
    discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("scalar conversion should parse");
}

#[test]
fn toml_integer_scalar_is_accepted_for_unknown_keys() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "answer = 42\n[rules]\nanchors = 'disable'\n");
    discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("integer conversion should parse");
}

#[test]
fn toml_custom_rule_entries_are_preserved() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        cfg.clone(),
        "[rules]\nanchors = 'disable'\n[rules.custom-rule]\nflag = true\n",
    );
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("custom TOML rules should still parse");

    assert!(
        ctx.config
            .rule_names()
            .iter()
            .any(|name| name == "custom-rule")
    );
    assert!(ctx.config.rule_names().iter().any(|name| name == "anchors"));
}

#[test]
fn toml_float_value_for_locale_reports_string_error() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "locale = 1.25\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("invalid config: locale should be a string"));
}

#[test]
fn invalid_project_pyproject_toml_errors_in_discover_config() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(PathBuf::from("/repo/pyproject.toml"), "[tool.ryl\n")
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let err = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn invalid_project_pyproject_toml_errors_in_discover_per_file() {
    let td = tempdir().unwrap();
    let root = td.path();
    std::fs::write(root.join("pyproject.toml"), "[tool.ryl\n").unwrap();
    std::fs::write(root.join("file.yaml"), "a: 1\n").unwrap();
    let err = discover_per_file(&root.join("file.yaml")).unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn explicit_invalid_pyproject_toml_reports_parse_error() {
    let pyproject = PathBuf::from("/repo/pyproject.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(pyproject.clone(), "[tool.ryl\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(pyproject),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn yaml_extends_toml_is_rejected() {
    let cfg = PathBuf::from("/repo/.yamllint");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "extends: .ryl.toml\n")
        .with_file(PathBuf::from("/repo/.ryl.toml"), "locale = 'en_US.UTF-8'\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("extends cannot reference TOML configuration"));
}
