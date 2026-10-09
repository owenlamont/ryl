use std::fs;
use std::path::Path;

use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::config_schema::{MarkerTarget, QuoteStyleTarget};
use ryl::migrate::{
    MigrateOptions, OutputMode, SourceCleanup, WriteMode, migrate_configs,
};
use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

fn migrate(dir: &Path, legacy: &str) -> YamlLintConfig {
    fs::write(dir.join(".yamllint"), legacy).unwrap();
    migrate_configs(&MigrateOptions {
        project_root: Some(dir.to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Write,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Delete,
    })
    .unwrap();
    discover_config(
        &[],
        &Overrides {
            config_file: Some(dir.join(".ryl.toml")),
            config_data: None,
        },
    )
    .unwrap()
    .config
}

#[test]
fn migrated_disabled_rules_stop_format_enforcing_them() {
    let td = tempdir().unwrap();
    let cfg = migrate(
        td.path(),
        "extends: default\nrules:\n  document-start: disable\n  quoted-strings: disable\n",
    );
    assert_eq!(
        cfg.format().targets().document_start,
        MarkerTarget::Preserve
    );
    assert_eq!(
        cfg.format().targets().quote_style,
        QuoteStyleTarget::Preserve
    );
    assert_eq!(cfg.format().targets().document_end, MarkerTarget::Preserve);
    let toml = fs::read_to_string(td.path().join(".ryl.toml")).unwrap();
    let format = toml::from_str::<toml::Table>(&toml).unwrap()["format"].clone();
    assert_eq!(
        format,
        toml::toml! { quote-style = "preserve" }.into(),
        "{toml}"
    );

    fs::write(td.path().join("a.yaml"), "a: \"x\"\n").unwrap();
    let (code, stdout, stderr) =
        run(ryl(td.path()).arg("format").arg("--check").arg(td.path()));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
}

#[test]
fn migrated_unenforced_options_are_preserved() {
    let td = tempdir().unwrap();
    let cfg = migrate(
        td.path(),
        "rules:\n  document-start: {present: false}\n  document-end: {present: false}\n  \
         comments: {require-starting-space: false}\n  \
         quoted-strings: enable\n",
    );
    assert_eq!(
        cfg.format().targets().document_start,
        MarkerTarget::Preserve
    );
    assert_eq!(cfg.format().targets().document_end, MarkerTarget::Preserve);
    assert_eq!(
        cfg.format().targets().comment_starting_space,
        MarkerTarget::Preserve
    );
    assert_eq!(cfg.format().targets().quote_style, QuoteStyleTarget::Single);
}

#[test]
fn migrated_rules_absent_from_a_non_extending_config_are_preserved() {
    let td = tempdir().unwrap();
    let cfg = migrate(td.path(), "rules:\n  line-length: enable\n");
    assert_eq!(
        cfg.format().targets().document_start,
        MarkerTarget::Preserve
    );
    assert_eq!(
        cfg.format().targets().quote_style,
        QuoteStyleTarget::Preserve
    );
    assert_eq!(
        cfg.format().targets().comment_starting_space,
        MarkerTarget::Preserve
    );
}

#[test]
fn migrated_enabled_rules_write_no_format_table() {
    let td = tempdir().unwrap();
    migrate(
        td.path(),
        "rules:\n  document-start: enable\n  document-end: enable\n  comments: enable\n  \
         quoted-strings: enable\n",
    );
    let toml = fs::read_to_string(td.path().join(".ryl.toml")).unwrap();
    assert!(!toml.contains("[format]"), "{toml}");
}
