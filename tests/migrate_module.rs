use std::fs;
use std::path::PathBuf;

use ryl::migrate::{
    MigrateOptions, MigrationEntry, OutputMode, SourceCleanup, WriteMode,
    apply_migration_entries, migrate_configs,
};
use tempfile::tempdir;

#[test]
fn migrate_errors_when_root_is_missing() {
    let opts = MigrateOptions {
        root: PathBuf::from("/no/such/path/for/ryl-migrate"),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let err = migrate_configs(&opts).unwrap_err();
    assert!(err.contains("migrate root does not exist"));
}

#[test]
fn migrate_single_non_config_file_returns_no_entries() {
    let td = tempdir().unwrap();
    let file = td.path().join("notes.txt");
    fs::write(&file, "hello").unwrap();
    let opts = MigrateOptions {
        root: file,
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert!(res.entries.is_empty());
    assert!(res.cleanup_only_sources.is_empty());
}

#[test]
fn migrate_single_yaml_config_file_path_returns_entry() {
    let td = tempdir().unwrap();
    let file = td.path().join(".yamllint");
    fs::write(&file, "rules: { document-start: disable }\n").unwrap();
    let opts = MigrateOptions {
        root: file.clone(),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert_eq!(res.entries[0].source, file);
    assert!(res.cleanup_only_sources.is_empty());
}

#[test]
fn migrate_write_mode_with_delete_removes_source_file() {
    let td = tempdir().unwrap();
    let source = td.path().join(".yamllint");
    fs::write(&source, "rules: { document-start: disable }\n").unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Delete,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert!(!source.exists());
    assert!(td.path().join(".ryl.toml").exists());
    assert!(res.cleanup_only_sources.is_empty());
}

#[test]
fn migrate_write_mode_with_keep_preserves_source_file() {
    let td = tempdir().unwrap();
    let source = td.path().join(".yamllint");
    fs::write(&source, "rules: { document-start: disable }\n").unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert!(source.exists());
    assert!(td.path().join(".ryl.toml").exists());
    assert!(res.cleanup_only_sources.is_empty());
}

#[test]
fn migrate_write_mode_with_rename_suffix_renames_source_file() {
    let td = tempdir().unwrap();
    let source = td.path().join(".yamllint");
    fs::write(&source, "rules: { document-start: disable }\n").unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::RenameSuffix(".bak".to_string()),
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert!(!source.exists());
    assert!(td.path().join(".yamllint.bak").exists());
    assert!(td.path().join(".ryl.toml").exists());
    assert!(res.cleanup_only_sources.is_empty());
}

#[test]
fn migrate_warns_on_multiple_same_directory_configs() {
    let td = tempdir().unwrap();
    fs::write(td.path().join(".yamllint.yaml"), "rules: {}\n").unwrap();
    fs::write(td.path().join(".yamllint.yml"), "rules: {}\n").unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert_eq!(res.cleanup_only_sources.len(), 1);
    assert_eq!(res.warnings.len(), 1);
    assert!(res.warnings[0].contains("lower-precedence"));
}

#[test]
fn migrate_errors_on_invalid_yaml_config_data() {
    let td = tempdir().unwrap();
    fs::write(td.path().join(".yamllint"), "rules: {\n").unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let err = migrate_configs(&opts).unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn migrate_errors_when_generated_toml_conversion_fails() {
    let td = tempdir().unwrap();
    fs::write(
        td.path().join(".yamllint"),
        "rules: { custom-rule: { opt: ~ } }\n",
    )
    .unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let err = migrate_configs(&opts).unwrap_err();
    assert!(err.contains("cannot convert null values to TOML"));
}

#[test]
fn migrate_write_errors_when_target_path_is_directory() {
    let td = tempdir().unwrap();
    fs::write(td.path().join(".yamllint"), "rules: {}\n").unwrap();
    fs::create_dir(td.path().join(".ryl.toml")).unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let err = migrate_configs(&opts).unwrap_err();
    assert!(err.contains("failed to write migrated config"));
}

#[test]
fn migrate_write_errors_when_rename_destination_is_invalid() {
    let td = tempdir().unwrap();
    fs::write(td.path().join(".yamllint"), "rules: {}\n").unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::RenameSuffix("/missing/subdir".to_string()),
    };
    let err = migrate_configs(&opts).unwrap_err();
    assert!(err.contains("failed to rename migrated source config"));
}

#[test]
fn apply_entries_delete_mode_propagates_delete_failures() {
    let td = tempdir().unwrap();
    let target = td.path().join(".ryl.toml");
    let source = td.path().join(".yamllint");
    let entries = vec![MigrationEntry {
        source: source.clone(),
        target,
        toml: "[rules]\n".to_string(),
    }];
    let err =
        apply_migration_entries(&entries, &[], &SourceCleanup::Delete).unwrap_err();
    assert!(err.contains("failed to delete migrated source config"));
}

#[test]
fn apply_entries_delete_mode_cleans_up_cleanup_only_sources() {
    let td = tempdir().unwrap();
    let skipped = td.path().join(".yamllint.yml");
    fs::write(&skipped, "rules: {}\n").unwrap();
    apply_migration_entries(
        &[],
        std::slice::from_ref(&skipped),
        &SourceCleanup::Delete,
    )
    .unwrap();
    assert!(!skipped.exists());
}

#[test]
fn migrate_delete_mode_removes_lower_precedence_skipped_configs() {
    let td = tempdir().unwrap();
    let primary = td.path().join(".yamllint");
    let skipped = td.path().join(".yamllint.yml");
    fs::write(&primary, "rules: { document-start: disable }\n").unwrap();
    fs::write(&skipped, "rules: {}\n").unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Delete,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert_eq!(res.cleanup_only_sources, vec![skipped.clone()]);
    assert!(!primary.exists());
    assert!(!skipped.exists());
    assert!(td.path().join(".ryl.toml").exists());
}

#[test]
fn migrate_rename_mode_renames_lower_precedence_skipped_configs() {
    let td = tempdir().unwrap();
    let primary = td.path().join(".yamllint");
    let skipped = td.path().join(".yamllint.yml");
    fs::write(&primary, "rules: { document-start: disable }\n").unwrap();
    fs::write(&skipped, "rules: {}\n").unwrap();
    let opts = MigrateOptions {
        root: td.path().to_path_buf(),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::RenameSuffix(".bak".to_string()),
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert_eq!(res.cleanup_only_sources, vec![skipped.clone()]);
    assert!(!primary.exists());
    assert!(td.path().join(".yamllint.bak").exists());
    assert!(!skipped.exists());
    assert!(td.path().join(".yamllint.yml.bak").exists());
}
