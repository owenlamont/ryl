use std::fs;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::migrate::{
    MigrateOptions, MigrationSource, OutputMode, SourceCleanup, WriteMode,
    migrate_configs,
};
use tempfile::tempdir;

use crate::property::migration_strategy::{
    arb_hostile_source, arb_source, arb_source_timestamp_string,
};

fn options(root: &std::path::Path, tool: MigrationSource) -> MigrateOptions {
    MigrateOptions {
        from: Some(tool),
        project_root: Some(root.to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::IncludeToml,
        cleanup: SourceCleanup::Delete,
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/proptest-regressions/property_migrate.txt"))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn imported_configs_preserve_dimensions_sources_and_repeat_safely(source in arb_source()) {
        let dir = tempdir().unwrap();
        let path = dir.path().join(source.filename);
        fs::write(&path, &source.body).unwrap();
        let mut options = options(dir.path(), source.tool);
        let preview = migrate_configs(&options).unwrap();
        prop_assert_eq!(preview.entries.len(), 1, "{:?}", preview);
        let config = YamlLintConfig::from_toml_str(&preview.entries[0].toml).unwrap();
        prop_assert_eq!(ryl::format::indent_width(&config), source.indent);
        prop_assert_eq!(ryl::format::line_length(&config), source.width);
        prop_assert!(!config.enables_any_rule());
        prop_assert_eq!(fs::read_to_string(&path).unwrap(), source.body.clone());
        prop_assert!(!dir.path().join(".ryl.toml").exists());
        options.write_mode = WriteMode::Write;
        let written = migrate_configs(&options).unwrap();
        prop_assert_eq!(&written.entries[0].toml, &preview.entries[0].toml);
        prop_assert_eq!(fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(), preview.entries[0].toml.clone());
        prop_assert_eq!(fs::read_to_string(&path).unwrap(), source.body);
        prop_assert!(migrate_configs(&options).unwrap().entries.is_empty());
    }

    #[test]
    fn hostile_source_options_never_partially_write_or_remove_sources((tool, filename, body) in arb_hostile_source()) {
        let dir = tempdir().unwrap();
        let path = dir.path().join(filename);
        fs::write(&path, &body).unwrap();
        let mut options = options(&path, tool);
        options.write_mode = WriteMode::Write;
        let result = migrate_configs(&options);
        prop_assert_eq!(fs::read_to_string(&path).unwrap(), body);
        if let Ok(result) = result {
            if result.entries.is_empty() {
                prop_assert!(!result.warnings.is_empty());
                prop_assert!(!dir.path().join(".ryl.toml").exists());
            } else {
                prop_assert_eq!(result.entries.len(), 1);
                prop_assert!(YamlLintConfig::from_toml_str(&result.entries[0].toml).is_ok());
            }
        } else {
            prop_assert!(!dir.path().join(".ryl.toml").exists());
        }
    }

    #[test]
    fn duplicate_yaml_options_never_write_or_cleanup_sources(yamlfmt in any::<bool>(), first in 1_u8..9, second in 1_u8..9, quoted in any::<bool>(), flow in any::<bool>(), crlf in any::<bool>()) {
        let dir = tempdir().unwrap();
        let (tool, filename, key) = if yamlfmt { (MigrationSource::Yamlfmt, ".yamlfmt", "indent") } else { (MigrationSource::Prettier, ".prettierrc.yaml", "tabWidth") };
        let second_key = if quoted { format!("'{key}'") } else { key.to_owned() };
        let body = if flow { format!("{{{key}: {first}, {second_key}: {second}}}") } else { format!("{key}: {first}\n{second_key}: {second}\n") };
        let body = if yamlfmt { format!("formatter:\n{}", body.lines().map(|line| format!("  {line}\n")).collect::<String>()) } else { body };
        let body = if crlf { body.replace('\n', "\r\n") } else { body };
        let path = dir.path().join(filename);
        fs::write(&path, &body).unwrap();
        let mut options = options(dir.path(), tool);
        options.write_mode = WriteMode::Write;
        prop_assert!(migrate_configs(&options).is_err());
        prop_assert!(!dir.path().join(".ryl.toml").exists());
        prop_assert_eq!(fs::read_to_string(path).unwrap(), body);
    }

    #[test]
    fn source_timestamp_uncertainty_never_becomes_a_string_cleanup_candidate(value in arb_source_timestamp_string()) {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".yamlfmt");
        let body = format!("include:\n  - {value}\n");
        fs::write(&path, &body).unwrap();
        let mut options = options(dir.path(), MigrationSource::Yamlfmt);
        options.write_mode = WriteMode::Write;
        let result = migrate_configs(&options).unwrap();
        prop_assert!(result.entries.is_empty());
        prop_assert!(!result.warnings.is_empty());
        prop_assert_eq!(fs::read_to_string(&path).unwrap(), body);
        prop_assert!(!dir.path().join(".ryl.toml").exists());
        fs::write(&path, format!("include: ['{value}']\n")).unwrap();
        prop_assert_eq!(migrate_configs(&options).unwrap().entries.len(), 1);
        prop_assert!(!path.exists());
    }

    #[test]
    fn formatter_only_imports_never_shadow_inherited_lint(source in arb_source()) {
        let dir = tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        let path = child.join(source.filename);
        let input = child.join("input.yaml");
        fs::write(&path, &source.body).unwrap();
        fs::write(&input, "flag: yes\n").unwrap();
        fs::write(dir.path().join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
        let overrides = ryl::config::Overrides { config_file: None, config_data: None };
        let before = ryl::config::discover_config(std::slice::from_ref(&input), &overrides).unwrap().config;
        let mut options = options(dir.path(), source.tool);
        options.write_mode = WriteMode::Write;
        let result = migrate_configs(&options).unwrap();
        prop_assert_eq!(result.entries.len(), 1);
        prop_assert!(!child.join(".ryl.toml").exists());
        prop_assert_eq!(fs::read_to_string(path).unwrap(), source.body);
        prop_assert!(result.warnings.iter().any(|warning| warning.contains("shadow inherited")));
        let after = ryl::config::discover_config(std::slice::from_ref(&input), &overrides).unwrap().config;
        let before = ryl::lint_str("flag: yes\n", &input, &before, dir.path());
        prop_assert!(!before.is_empty());
        prop_assert_eq!(before, ryl::lint_str("flag: yes\n", &input, &after, dir.path()));
    }


    #[test]
    fn explicit_formatter_files_keep_colocated_lint_active(source in arb_source()) {
        let dir = tempdir().unwrap();
        let path = dir.path().join(source.filename);
        let input = dir.path().join("input.yaml");
        let lint = dir.path().join(".yamllint");
        fs::write(&path, &source.body).unwrap();
        fs::write(&input, "flag: yes\n").unwrap();
        fs::write(&lint, "rules: {truthy: enable}\n").unwrap();
        let overrides = ryl::config::Overrides { config_file: None, config_data: None };
        let before = ryl::config::discover_config(std::slice::from_ref(&input), &overrides).unwrap().config;
        let mut options = options(&path, source.tool);
        options.write_mode = WriteMode::Write;
        let result = migrate_configs(&options).unwrap();
        prop_assert_eq!(result.entries.len(), 1);
        prop_assert!(lint.exists());
        prop_assert_eq!(fs::read_to_string(path).unwrap(), source.body);
        let after = ryl::config::discover_config(std::slice::from_ref(&input), &overrides).unwrap().config;
        let before = ryl::lint_str("flag: yes\n", &input, &before, dir.path());
        prop_assert!(!before.is_empty());
        prop_assert_eq!(before, ryl::lint_str("flag: yes\n", &input, &after, dir.path()));
        prop_assert_eq!(ryl::format::indent_width(&after), source.indent);
        prop_assert_eq!(ryl::format::line_length(&after), source.width);
    }

    #[test]
    fn inherited_yamlfix_cleanup_is_independent_of_root_spelling(width in 20_u16..161, indent in 1_u8..9, path_kind in 0_u8..3, rename in any::<bool>()) {
        let dir = tempdir().unwrap();
        let logical = tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        let parent_source = dir.path().join("yamlfix.toml");
        let child_source = child.join(".yamlfix.toml");
        fs::write(&parent_source, format!("line_length={width}\n")).unwrap();
        fs::write(&child_source, format!("indent_mapping={indent}\n")).unwrap();
        let sentinel = logical.path().join("yamlfix.toml");
        let sentinel_body = format!("line_length={}\n", width + 1);
        fs::write(&sentinel, &sentinel_body).unwrap();
        let root = if path_kind == 1 { dir.path().join("nested/..") } else { dir.path().to_path_buf() };
        #[cfg(unix)]
        let root = if path_kind == 2 {
            std::os::unix::fs::symlink(dir.path().join("nested"), logical.path().join("link")).unwrap();
            logical.path().join("link/..")
        } else { root };
        let mut options = options(&root, MigrationSource::Yamlfix);
        options.write_mode = WriteMode::Write;
        options.cleanup = if rename { SourceCleanup::RenameSuffix(".bak".to_owned()) } else { SourceCleanup::Delete };
        let result = migrate_configs(&options).unwrap();
        prop_assert_eq!(result.entries.len(), 2);
        prop_assert!(!parent_source.exists());
        prop_assert!(!child_source.exists());
        prop_assert_eq!(dir.path().join("yamlfix.toml.bak").exists(), rename);
        prop_assert_eq!(child.join(".yamlfix.toml.bak").exists(), rename);
        let config = YamlLintConfig::from_toml_str(&fs::read_to_string(child.join(".ryl.toml")).unwrap()).unwrap();
        prop_assert_eq!(ryl::format::indent_width(&config), indent);
        prop_assert_eq!(ryl::format::line_length(&config), width);
        prop_assert_eq!(fs::read_to_string(sentinel).unwrap(), sentinel_body);
    }

}
