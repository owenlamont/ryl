use std::path::PathBuf;

use ryl::config::{Overrides, YamlLintConfig, discover_config, discover_config_with};

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

#[test]
fn inline_extends_mapping_is_ignored() {
    let yaml = "extends:\n  invalid: true\nrules: {}\n";
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect("inline config should parse");
    assert!(ctx.config.rule_names().is_empty());
}

#[test]
fn extends_requires_env_error() {
    let err = YamlLintConfig::from_yaml_str("extends: child.yml\n")
        .expect_err("extends without env should error");
    assert!(err.contains("requires filesystem access"));
}

#[test]
fn extends_value_non_string_is_ignored() {
    let cfg = YamlLintConfig::from_yaml_str("extends: 123\nrules: {}\n")
        .expect("non-string extends should be ignored");
    assert!(cfg.rule_names().is_empty());
}

#[test]
fn inline_extends_sequence_skips_non_strings() {
    let yaml = "extends: [default, 1]\n";
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect("extends should allow mixed types");
    assert!(ctx.config.rule_names().iter().any(|r| r == "anchors"));
}

#[test]
fn extends_sequence_missing_entry_errors() {
    let root = PathBuf::from("/workspace");
    let child = root.join("child.yml");
    let base = root.join("base.yml");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(child.clone(), "extends: [base.yml, missing.yml]\n")
        .with_file(base.clone(), "rules: {}\n")
        .with_exists(child.clone())
        .with_exists(base);

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(child),
            config_data: None,
        },
        &env,
    )
    .expect_err("missing extended file should error");
    assert!(err.contains("failed to read extended config"));
}

#[test]
fn extends_invalid_yaml_propagates_error() {
    let root = PathBuf::from("/workspace");
    let child = root.join("child.yml");
    let base = root.join("base.yml");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(child.clone(), "extends: base.yml\n")
        .with_file(base.clone(), "- not mapping\n")
        .with_exists(child)
        .with_exists(base.clone());

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(root.join("child.yml")),
            config_data: None,
        },
        &env,
    )
    .expect_err("invalid yaml in extended file should propagate");
    assert!(err.contains("invalid config"));
}

#[test]
fn extend_prefers_absolute_paths() {
    let root = PathBuf::from("/workspace");
    let abs = PathBuf::from("/configs/base.yml");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(abs.clone(), "rules:\n  abs_rule: enable\n")
        .with_exists(abs.clone());

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(format!("extends: {}\n", abs.display())),
        },
        &env,
    )
    .expect("absolute extends should resolve");
    assert!(ctx.config.rule_names().iter().any(|r| r == "abs_rule"));
}

#[test]
fn extend_falls_back_to_env_cwd_when_base_dir_missing_entry() {
    let cwd = PathBuf::from("/env-cwd");
    let project = PathBuf::from("/project");
    let cli_cfg = project.join("child.yml");
    let env = FakeEnv::new()
        .with_cwd(cwd.clone())
        .with_file(cli_cfg.clone(), "extends: config.yml\n")
        .with_exists(cli_cfg.clone())
        .with_file(cwd.join("config.yml"), "rules:\n  cwd_rule: enable\n")
        .with_exists(cwd.join("config.yml"));

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cli_cfg),
            config_data: None,
        },
        &env,
    )
    .expect("extend should fall back to cwd");
    assert!(ctx.config.rule_names().iter().any(|r| r == "cwd_rule"));
}

#[test]
fn extend_empty_entry_uses_candidate_path() {
    let root = PathBuf::from("/workspace");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(PathBuf::from(""), "rules: {}\n");

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("extends: ''\n".into()),
        },
        &env,
    )
    .expect("empty extends should resolve using candidate path");
    assert!(ctx.config.rule_names().is_empty());
}

#[test]
fn extend_relative_entry_uses_base_directory() {
    let project = PathBuf::from("/workspace/project");
    let child = project.join("child.yml");
    let base = project.join("base.yml");
    let env = FakeEnv::new()
        .with_cwd(project.clone())
        .with_file(child.clone(), "extends: base.yml\n")
        .with_exists(child.clone())
        .with_file(base.clone(), "rules: { base_rule: enable }\n")
        .with_exists(base.clone());

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(child),
            config_data: None,
        },
        &env,
    )
    .expect("relative extends should resolve using config directory");
    assert!(ctx.config.rule_names().iter().any(|r| r == "base_rule"));
}
