use std::fs;

use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::fix::{
    apply_safe_fixes, apply_safe_fixes_in_place, apply_safe_fixes_to_files,
};
use tempfile::tempdir;

fn config(yaml: &str) -> YamlLintConfig {
    YamlLintConfig::from_yaml_str(yaml).expect("config parses")
}

#[test]
fn apply_safe_fixes_in_place_reports_no_change() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value\n").unwrap();
    let cfg = config(
        "rules:\n  comments: enable\n  new-lines: enable\n  new-line-at-end-of-file: enable\n",
    );

    let changed =
        apply_safe_fixes_in_place(&file, &cfg, dir.path()).expect("fix succeeds");

    assert!(!changed);
    assert_eq!(fs::read_to_string(&file).unwrap(), "key: value\n");
}

#[test]
fn apply_safe_fixes_in_place_writes_changes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    let cfg = config("rules:\n  comments: enable\n  new-line-at-end-of-file: enable\n");

    let changed =
        apply_safe_fixes_in_place(&file, &cfg, dir.path()).expect("fix succeeds");

    assert!(changed);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "key: value  # comment\n"
    );
}

#[test]
fn apply_safe_fixes_preserves_existing_newline_style_for_final_newline() {
    let cfg = config("rules:\n  comments: enable\n  new-line-at-end-of-file: enable\n");

    let fixed = apply_safe_fixes(
        "key: value #comment\r\nnext: value",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );

    assert_eq!(fixed, "key: value  # comment\r\nnext: value\r\n");
}

#[test]
fn apply_safe_fixes_defaults_to_unix_newline_when_none_present() {
    let cfg = config("rules:\n  new-line-at-end-of-file: enable\n");

    let fixed = apply_safe_fixes(
        "key: value",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );

    assert_eq!(fixed, "key: value\n");
}

#[test]
fn apply_safe_fixes_skips_ignored_rules() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        dir.path().join(".yamllint"),
        "rules:\n  comments:\n    level: error\n    ignore: input.yaml\n  new-line-at-end-of-file: enable\n",
    )
    .unwrap();

    let ctx = discover_config(std::slice::from_ref(&file), &Overrides::default())
        .expect("config discovers");

    let fixed = apply_safe_fixes("key: value #comment", &ctx.config, &file, dir.path());

    assert_eq!(fixed, "key: value #comment\n");
}

#[test]
fn apply_safe_fixes_in_place_returns_read_error_for_missing_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("missing.yaml");
    let cfg = config("rules:\n  comments: enable\n");

    let err = apply_safe_fixes_in_place(&file, &cfg, dir.path())
        .expect_err("missing file should fail");

    assert!(err.contains("failed to read"));
}

#[test]
fn apply_safe_fixes_in_place_returns_write_error_for_read_only_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms).unwrap();

    let cfg = config("rules:\n  comments: enable\n  new-line-at-end-of-file: enable\n");

    let err = apply_safe_fixes_in_place(&file, &cfg, dir.path())
        .expect_err("read-only file should fail");

    assert!(err.contains("failed to write fixed file"));
}

#[test]
fn apply_safe_fixes_runs_newline_normalization() {
    let cfg = config("rules:\n  new-lines:\n    type: unix\n");

    let fixed = apply_safe_fixes(
        "key: value\r\nnext: value\r\n",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );

    assert_eq!(fixed, "key: value\nnext: value\n");
}

#[test]
fn apply_safe_fixes_preserves_unix_newline_for_final_newline_when_present() {
    let cfg = config("rules:\n  new-line-at-end-of-file: enable\n");

    let fixed = apply_safe_fixes(
        "key: value\nnext: value",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );

    assert_eq!(fixed, "key: value\nnext: value\n");
}

#[test]
fn apply_safe_fixes_to_files_updates_each_entry() {
    let dir = tempdir().unwrap();
    let first = dir.path().join("first.yaml");
    let second = dir.path().join("second.yaml");
    fs::write(&first, "key: value #comment").unwrap();
    fs::write(&second, "alpha: beta").unwrap();
    let cfg = config("rules:\n  comments: enable\n  new-line-at-end-of-file: enable\n");
    let files = vec![
        (first.clone(), dir.path().to_path_buf(), cfg.clone()),
        (second.clone(), dir.path().to_path_buf(), cfg),
    ];

    let stats = apply_safe_fixes_to_files(&files).expect("fixes succeed");

    assert_eq!(
        fs::read_to_string(&first).unwrap(),
        "key: value  # comment\n"
    );
    assert_eq!(fs::read_to_string(&second).unwrap(), "alpha: beta\n");
    assert_eq!(stats.changed_files, 2);
}

#[test]
fn apply_safe_fixes_runs_comments_indentation_and_commas() {
    let cfg = config(
        "rules:\n  comments-indentation: enable\n  commas: enable\n  new-line-at-end-of-file: disable\n",
    );

    let fixed = apply_safe_fixes(
        "items: [1 ,2]\n # wrong\n  next: value\n",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );

    assert_eq!(fixed, "items: [1, 2]\n  # wrong\n  next: value\n");
}

#[test]
fn apply_safe_fixes_runs_flow_collection_spacing_fixes() {
    let cfg = config(
        "rules:\n  braces:\n    min-spaces-inside-empty: 1\n    max-spaces-inside-empty: 1\n  brackets:\n    min-spaces-inside-empty: 1\n    max-spaces-inside-empty: 1\n  new-line-at-end-of-file: disable\n",
    );

    let fixed = apply_safe_fixes(
        "mapping: {  key: value   }\nsequence: []\n",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );

    assert_eq!(fixed, "mapping: {key: value}\nsequence: [ ]\n");
}
