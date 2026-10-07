use std::path::PathBuf;

use ryl::config::{Overrides, discover_config_with};
use ryl::config_schema::{LEGACY_YAML_SOURCES, LegacyYamlSource};

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

const RULES: &str = "rules: {anchors: enable}\n";

fn notices(env: &FakeEnv, overrides: &Overrides) -> Vec<String> {
    discover_config_with(&[PathBuf::from("/proj")], overrides, env)
        .expect("config loads")
        .notices
}

#[test]
fn legacy_yaml_sources_table_is_pinned() {
    let rows: Vec<_> = LEGACY_YAML_SOURCES
        .iter()
        .map(|row| (row.source, row.deprecated_since, row.removed_in))
        .collect();
    assert_eq!(
        rows,
        [
            (LegacyYamlSource::Project, "0.25.0", None),
            (LegacyYamlSource::ConfigFile, "0.25.0", None),
            (LegacyYamlSource::ConfigData, "0.25.0", None),
            (LegacyYamlSource::EnvVar, "0.25.0", None),
            (LegacyYamlSource::UserGlobal, "0.25.0", None),
        ]
    );
}

#[test]
fn config_file_yaml_names_the_file_migration() {
    let env = FakeEnv::new()
        .with_cwd("/proj")
        .with_file("/proj/ci/lint.yaml", RULES);
    let overrides = Overrides {
        config_file: Some(PathBuf::from("/proj/ci/lint.yaml")),
        config_data: None,
    };
    assert_eq!(
        notices(&env, &overrides),
        [
            "warning: /proj/ci/lint.yaml: yamllint YAML config is deprecated; run `ryl \
             --migrate-configs --migrate-write --migrate-root /proj/ci/lint.yaml` and pass \
             the TOML it writes to `-c`"
        ]
    );
}

#[test]
fn config_data_yaml_points_at_inline_toml() {
    let env = FakeEnv::new().with_cwd("/proj");
    let overrides = Overrides {
        config_file: None,
        config_data: Some(RULES.into()),
    };
    assert_eq!(
        notices(&env, &overrides),
        [
            "warning: -d/--config-data: yamllint YAML config is deprecated; pass inline \
             TOML to `-d` instead"
        ]
    );
}

#[test]
fn config_data_toml_loads_without_a_notice() {
    let env = FakeEnv::new().with_cwd("/proj");
    let overrides = Overrides {
        config_file: None,
        config_data: Some("lint.rules.anchors = \"enable\"".into()),
    };
    let ctx = discover_config_with(&[], &overrides, &env).expect("inline TOML loads");
    assert!(ctx.notices.is_empty(), "{:?}", ctx.notices);
    assert!(ctx.config.rule_names().iter().any(|name| name == "anchors"));
}

#[test]
fn env_var_yaml_names_the_variable() {
    let env = FakeEnv::new()
        .with_cwd("/proj")
        .with_var("YAMLLINT_CONFIG_FILE", "/cfg/lint.yml")
        .with_file("/cfg/lint.yml", RULES);
    assert_eq!(
        notices(&env, &Overrides::default()),
        [
            "warning: /cfg/lint.yml: yamllint YAML config is deprecated; \
             YAMLLINT_CONFIG_FILE is deprecated too: run `ryl --migrate-configs \
             --migrate-write --migrate-root /cfg/lint.yml` and pass the TOML it writes to \
             `-c`"
        ]
    );
}

#[test]
fn yamllint_user_global_names_migrate_user_config() {
    let env = FakeEnv::new()
        .with_cwd("/proj")
        .with_var("XDG_CONFIG_HOME", "/xdg")
        .with_file("/xdg/yamllint/config", RULES);
    assert_eq!(
        notices(&env, &Overrides::default()),
        [
            "warning: /xdg/yamllint/config: yamllint YAML config is deprecated; run `ryl \
             --migrate-user-config` to convert it to ryl's own user config"
        ]
    );
}

#[test]
fn config_data_toml_with_an_unknown_key_reports_the_toml_error() {
    let env = FakeEnv::new().with_cwd("/proj");
    let overrides = Overrides {
        config_file: None,
        config_data: Some("no-such-key = 1".into()),
    };
    let err = discover_config_with(&[], &overrides, &env).expect_err("unknown key");
    assert!(err.contains("no-such-key"), "{err}");
}
