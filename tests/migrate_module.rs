use std::fs;
use std::path::PathBuf;

use ryl::migrate::{
    MigrateOptions, MigrationEntry, OutputMode, SourceCleanup, UserConfigMigration,
    WriteMode, apply_migration_entries, migrate_configs,
};
use tempfile::tempdir;

#[test]
fn migrate_errors_when_root_is_missing() {
    let opts = MigrateOptions {
        project_root: Some(PathBuf::from("/no/such/path/for/ryl-migrate")),
        user_config: None,
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
        project_root: Some(file),
        user_config: None,
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
        project_root: Some(file.clone()),
        user_config: None,
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
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
    // Both enable a rule so the only warning is the lower-precedence dedup, not the
    // separate "enables no rules" warning (exercised by its own test below).
    fs::write(
        td.path().join(".yamllint.yaml"),
        "rules:\n  anchors: enable\n",
    )
    .unwrap();
    fs::write(
        td.path().join(".yamllint.yml"),
        "rules:\n  anchors: enable\n",
    )
    .unwrap();
    let opts = MigrateOptions {
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
fn migrate_warns_when_migrated_config_enables_no_rules() {
    let td = tempdir().unwrap();
    fs::write(td.path().join(".yamllint"), "rules: {}\n").unwrap();
    let opts = MigrateOptions {
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert_eq!(res.warnings.len(), 1);
    assert!(
        res.warnings[0].contains("enables no rules"),
        "a migrated rule-less config must warn it will not lint: {:?}",
        res.warnings,
    );
}

#[test]
fn migrate_errors_on_invalid_yaml_config_data() {
    let td = tempdir().unwrap();
    fs::write(td.path().join(".yamllint"), "rules: {\n").unwrap();
    let opts = MigrateOptions {
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::RenameSuffix("/missing/subdir".to_string()),
    };
    let err = migrate_configs(&opts).unwrap_err();
    assert!(err.contains("failed to rename migrated source config"));
}

#[test]
fn migrate_user_config_writes_target_creating_missing_dir() {
    let td = tempdir().unwrap();
    let source = td.path().join("yamllint").join("config");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "rules: { key-duplicates: enable }\n").unwrap();
    // The ryl/ target dir does not exist yet; the write must create it.
    let target = td.path().join("ryl").join("ryl.toml");
    let opts = MigrateOptions {
        project_root: None,
        user_config: Some(UserConfigMigration {
            source: source.clone(),
            target: target.clone(),
        }),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    assert!(
        target.exists(),
        "user-global target written, creating ryl/ dir"
    );
    assert!(source.exists(), "Keep preserves the yamllint source");
}

#[test]
fn migrate_user_config_absent_source_yields_no_entry() {
    let td = tempdir().unwrap();
    let opts = MigrateOptions {
        project_root: None,
        user_config: Some(UserConfigMigration {
            source: td.path().join("yamllint").join("config"),
            target: td.path().join("ryl").join("ryl.toml"),
        }),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert!(res.entries.is_empty());
}

#[test]
fn migrate_user_config_propagates_parse_error() {
    let td = tempdir().unwrap();
    let source = td.path().join("yamllint").join("config");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "rules: {\n").unwrap();
    let opts = MigrateOptions {
        project_root: None,
        user_config: Some(UserConfigMigration {
            source,
            target: td.path().join("ryl").join("ryl.toml"),
        }),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let err = migrate_configs(&opts).unwrap_err();
    assert!(err.contains("failed to parse config data"), "got: {err}");
}

#[test]
fn migrate_project_skips_when_ryl_native_config_exists() {
    let td = tempdir().unwrap();
    fs::write(
        td.path().join(".yamllint"),
        "rules: { key-duplicates: enable }\n",
    )
    .unwrap();
    // A pre-existing .ryl.toml must not be clobbered, even with --delete-old.
    fs::write(
        td.path().join(".ryl.toml"),
        "[rules]\nkey-duplicates = \"enable\"\n",
    )
    .unwrap();
    let opts = MigrateOptions {
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Delete,
    };
    let res = migrate_configs(&opts).unwrap();
    assert!(
        res.entries.is_empty(),
        "no migration when a ryl-native config exists"
    );
    assert!(
        res.warnings.iter().any(|w| w.contains("already exists")),
        "expected a collision warning: {:?}",
        res.warnings
    );
    assert!(
        td.path().join(".yamllint").exists(),
        "source preserved when migration is skipped"
    );
}

#[test]
fn migrate_user_config_skips_when_ryl_toml_exists() {
    let td = tempdir().unwrap();
    let source = td.path().join("yamllint").join("config");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "rules: { key-duplicates: enable }\n").unwrap();
    let ryl_dir = td.path().join("ryl");
    fs::create_dir_all(&ryl_dir).unwrap();
    // Only the non-hidden ryl.toml exists here (exercises the second collision candidate).
    fs::write(
        ryl_dir.join("ryl.toml"),
        "[rules]\nkey-duplicates = \"enable\"\n",
    )
    .unwrap();
    let opts = MigrateOptions {
        project_root: None,
        user_config: Some(UserConfigMigration {
            source: source.clone(),
            target: ryl_dir.join("ryl.toml"),
        }),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Delete,
    };
    let res = migrate_configs(&opts).unwrap();
    assert!(res.entries.is_empty());
    assert!(
        res.warnings.iter().any(|w| w.contains("already exists")),
        "expected a collision warning: {:?}",
        res.warnings
    );
    assert!(source.exists(), "yamllint source preserved when skipped");
}

#[test]
fn migrate_skipped_directory_does_not_delete_lower_precedence_siblings() {
    let td = tempdir().unwrap();
    let primary = td.path().join(".yamllint");
    let sibling = td.path().join(".yamllint.yml");
    fs::write(&primary, "rules: { key-duplicates: enable }\n").unwrap();
    fs::write(&sibling, "rules: {}\n").unwrap();
    // A pre-existing ryl-native config makes the whole directory's migration skip; its
    // lower-precedence siblings must not be deleted as a side effect.
    fs::write(
        td.path().join(".ryl.toml"),
        "[rules]\nkey-duplicates = \"enable\"\n",
    )
    .unwrap();
    let opts = MigrateOptions {
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Delete,
    };
    let res = migrate_configs(&opts).unwrap();
    assert!(res.entries.is_empty());
    assert!(
        res.cleanup_only_sources.is_empty(),
        "siblings must not be enqueued for cleanup when the directory is skipped"
    );
    assert!(primary.exists(), "primary preserved");
    assert!(
        sibling.exists(),
        "sibling preserved when the directory migration is skipped"
    );
}

#[test]
fn migrate_rename_refuses_to_overwrite_existing_backup() {
    let td = tempdir().unwrap();
    fs::write(
        td.path().join(".yamllint"),
        "rules: { key-duplicates: enable }\n",
    )
    .unwrap();
    fs::write(td.path().join(".yamllint.bak"), "previous backup\n").unwrap();
    let opts = MigrateOptions {
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::RenameSuffix(".bak".to_string()),
    };
    let err = migrate_configs(&opts).unwrap_err();
    assert!(
        err.contains("refusing to overwrite existing backup"),
        "got: {err}"
    );
    assert_eq!(
        fs::read_to_string(td.path().join(".yamllint.bak")).unwrap(),
        "previous backup\n",
        "an existing backup is preserved, not clobbered"
    );
    // Preflighted before writing, so no target is left behind for a retry to skip.
    assert!(
        !td.path().join(".ryl.toml").exists(),
        "no target written when a rename collision is detected"
    );
}

#[test]
fn apply_entries_refuses_to_overwrite_existing_target() {
    // create_new backstop: a target present at apply time (e.g. created after planning)
    // is never overwritten, and the source is left for cleanup only after a successful write.
    let td = tempdir().unwrap();
    let target = td.path().join("ryl.toml");
    fs::write(&target, "original\n").unwrap();
    let entries = vec![MigrationEntry {
        source: td.path().join(".yamllint"),
        target: target.clone(),
        toml: "[rules]\n".to_string(),
    }];
    let err = apply_migration_entries(&entries, &[], &SourceCleanup::Keep).unwrap_err();
    assert!(
        err.contains("failed to write migrated config"),
        "got: {err}"
    );
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "original\n",
        "an existing target is preserved, not overwritten"
    );
}

#[test]
fn migrate_user_config_inlines_top_level_ignore_from_file() {
    let td = tempdir().unwrap();
    let source = td.path().join("yamllint").join("config");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        &source,
        "rules:\n  key-duplicates: enable\nignore-from-file: ignores.txt\n",
    )
    .unwrap();
    fs::write(
        source.parent().unwrap().join("ignores.txt"),
        "build/\n*.gen.yaml\n",
    )
    .unwrap();
    let opts = MigrateOptions {
        project_root: None,
        user_config: Some(UserConfigMigration {
            source,
            target: td.path().join("ryl").join("ryl.toml"),
        }),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(res.entries.len(), 1);
    let toml = &res.entries[0].toml;
    // The relative path would dangle once the config moves to ryl/, so it is inlined.
    assert!(
        toml.contains("ignore = [")
            && toml.contains("build/")
            && toml.contains("*.gen.yaml"),
        "ignore-from-file must be inlined as ignore patterns: {toml}"
    );
    assert!(
        !toml.contains("ignore-from-file"),
        "the relative ignore-from-file path must not survive migration: {toml}"
    );
}

#[test]
fn migrate_user_config_refuses_rule_level_ignore_from_file() {
    let td = tempdir().unwrap();
    let source = td.path().join("yamllint").join("config");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        &source,
        "rules:\n  key-duplicates:\n    ignore-from-file: rignore.txt\n",
    )
    .unwrap();
    fs::write(source.parent().unwrap().join("rignore.txt"), "generated/\n").unwrap();
    let opts = MigrateOptions {
        project_root: None,
        user_config: Some(UserConfigMigration {
            source: source.clone(),
            target: td.path().join("ryl").join("ryl.toml"),
        }),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Delete,
    };
    let res = migrate_configs(&opts).unwrap();
    assert!(
        res.entries.is_empty(),
        "rule-level ignore-from-file is refused"
    );
    assert!(
        res.warnings
            .iter()
            .any(|w| w.contains("rule-level ignore-from-file")),
        "expected a rule-level refusal warning: {:?}",
        res.warnings
    );
    assert!(
        source.exists(),
        "source preserved when refused, even with --delete"
    );
}

#[test]
fn migrate_user_config_allows_absolute_rule_level_ignore_from_file() {
    let td = tempdir().unwrap();
    let source = td.path().join("yamllint").join("config");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    let abs_ignore = td.path().join("abs_ignore.txt");
    fs::write(&abs_ignore, "generated/\n").unwrap();
    // An absolute rule-level ignore-from-file survives the move, so it is not refused.
    fs::write(
        &source,
        format!(
            "rules:\n  key-duplicates:\n    level: error\n    ignore-from-file: {}\n",
            abs_ignore.display()
        ),
    )
    .unwrap();
    let opts = MigrateOptions {
        project_root: None,
        user_config: Some(UserConfigMigration {
            source,
            target: td.path().join("ryl").join("ryl.toml"),
        }),
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert_eq!(
        res.entries.len(),
        1,
        "an absolute rule-level ignore-from-file must migrate, not be refused"
    );
    assert!(
        res.entries[0].toml.contains("ignore-from-file"),
        "the absolute rule-level path is preserved: {}",
        res.entries[0].toml
    );
}

#[cfg(unix)]
#[test]
fn migrate_skips_symlinked_source() {
    let td = tempdir().unwrap();
    let real = td.path().join("real.yaml");
    fs::write(&real, "rules: { key-duplicates: enable }\n").unwrap();
    let link = td.path().join(".yamllint");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let opts = MigrateOptions {
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert!(res.entries.is_empty());
    assert!(
        res.warnings
            .iter()
            .any(|w| w.contains("refusing to follow a symlink")),
        "expected a symlink-refusal warning: {:?}",
        res.warnings
    );
    assert!(!td.path().join(".ryl.toml").exists());
}

#[cfg(unix)]
#[test]
fn migrate_user_config_skips_symlinked_target() {
    let td = tempdir().unwrap();
    let source = td.path().join("yamllint").join("config");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "rules: { key-duplicates: enable }\n").unwrap();
    let ryl_dir = td.path().join("ryl");
    fs::create_dir_all(&ryl_dir).unwrap();
    let victim = td.path().join("victim.toml");
    fs::write(&victim, "untouched\n").unwrap();
    let target = ryl_dir.join("ryl.toml");
    std::os::unix::fs::symlink(&victim, &target).unwrap();
    let opts = MigrateOptions {
        project_root: None,
        user_config: Some(UserConfigMigration {
            source,
            target: target.clone(),
        }),
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    };
    let res = migrate_configs(&opts).unwrap();
    assert!(res.entries.is_empty());
    assert!(
        res.warnings
            .iter()
            .any(|w| w.contains("refusing to follow a symlink")),
        "expected a symlink-refusal warning: {:?}",
        res.warnings
    );
    assert_eq!(
        fs::read_to_string(&victim).unwrap(),
        "untouched\n",
        "the symlink's target must not be clobbered"
    );
}

#[cfg(unix)]
#[test]
fn apply_entries_cleanup_skips_symlinked_source() {
    let td = tempdir().unwrap();
    let real = td.path().join("real.yaml");
    fs::write(&real, "rules: {}\n").unwrap();
    let link = td.path().join(".yamllint.yml");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    // RenameSuffix exercises both the rename preflight (which skips symlinks) and the
    // cleanup-time symlink guard; neither should rename through the symlink.
    apply_migration_entries(
        &[],
        std::slice::from_ref(&link),
        &SourceCleanup::RenameSuffix(".bak".to_string()),
    )
    .unwrap();
    assert!(
        fs::symlink_metadata(&link).is_ok(),
        "a symlinked source is preserved, not renamed through"
    );
    assert!(
        !td.path().join(".yamllint.yml.bak").exists(),
        "no backup is created for a skipped symlinked source"
    );
    assert!(real.exists(), "the symlink's real target is untouched");
}

#[test]
fn apply_entries_target_without_parent_does_not_panic() {
    // A parentless target (never produced by the planner) skips dir creation and falls
    // through to the write, which fails gracefully — proving the path cannot panic.
    let entries = vec![MigrationEntry {
        source: PathBuf::from("x"),
        target: PathBuf::from(""),
        toml: String::new(),
    }];
    let err = apply_migration_entries(&entries, &[], &SourceCleanup::Keep).unwrap_err();
    assert!(
        err.contains("failed to write migrated config"),
        "got: {err}"
    );
}

#[test]
fn apply_entries_errors_when_target_parent_cannot_be_created() {
    let td = tempdir().unwrap();
    // A regular file where a parent directory is needed makes create_dir_all fail.
    let blocker = td.path().join("blocker");
    fs::write(&blocker, "x").unwrap();
    let entries = vec![MigrationEntry {
        source: td.path().join(".yamllint"),
        target: blocker.join("ryl.toml"),
        toml: "[rules]\n".to_string(),
    }];
    let err = apply_migration_entries(&entries, &[], &SourceCleanup::Keep).unwrap_err();
    assert!(err.contains("failed to create directory"), "got: {err}");
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
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
        project_root: Some(td.path().to_path_buf()),
        user_config: None,
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
