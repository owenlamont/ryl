use std::fs;

use ryl::config::{Overrides, SourceKind, YamlLintConfig, discover_config};
use ryl::fix::{
    FixOutcome, apply_safe_fixes, apply_safe_fixes_in_place, apply_safe_fixes_to_files,
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

    let outcome =
        apply_safe_fixes_in_place(&file, &cfg, dir.path()).expect("fix succeeds");

    assert_eq!(outcome, FixOutcome::default());
    assert_eq!(fs::read_to_string(&file).unwrap(), "key: value\n");
}

#[test]
fn apply_safe_fixes_in_place_writes_changes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    let cfg = config("rules:\n  comments: enable\n  new-line-at-end-of-file: enable\n");

    let outcome =
        apply_safe_fixes_in_place(&file, &cfg, dir.path()).expect("fix succeeds");

    assert!(outcome.changed && outcome.skipped.is_empty());
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "key: value  # comment\n"
    );
}

#[test]
fn apply_safe_fixes_in_place_skips_and_reports_unparsable_file() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "a: *missing\n").unwrap();
    let cfg = config("rules:\n  trailing-spaces: enable\n");

    let outcome =
        apply_safe_fixes_in_place(&file, &cfg, dir.path()).expect("fix succeeds");

    assert!(!outcome.changed, "an unparsable file is not changed");
    assert_eq!(outcome.skipped.len(), 1, "one whole-file parse error");
    assert!(
        outcome.skipped[0].message.contains("unknown anchor"),
        "skip carries the parse error: {}",
        outcome.skipped[0].message
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "a: *missing\n",
        "a skipped file is left byte-for-byte unchanged"
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
        (
            first.clone(),
            dir.path().to_path_buf(),
            cfg.clone(),
            SourceKind::Yaml,
        ),
        (
            second.clone(),
            dir.path().to_path_buf(),
            cfg,
            SourceKind::Yaml,
        ),
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
        "items: [1 ,2]\n # wrong\nnext: value\n",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );

    assert_eq!(fixed, "items: [1, 2]\n# wrong\nnext: value\n");
}

#[test]
fn whitespace_fixers_bail_on_unparsable_input() {
    // The pipeline parse-gate normally keeps these fixers from seeing unparsable
    // input; called directly they must still bail, because `protected_scalar_lines`
    // cannot resolve scalar spans without a successful parse.
    let unparsable = "items: [1 ,2]\n # wrong\n  next: value\n";
    let empty_lines_cfg = ryl::rules::empty_lines::Config::resolve(&config(
        "rules:\n  empty-lines: enable\n",
    ));
    assert_eq!(
        ryl::rules::empty_lines::fix(unparsable, &empty_lines_cfg),
        None,
        "empty-lines fix bails when the buffer does not parse"
    );
    assert_eq!(
        ryl::rules::trailing_spaces::fix(unparsable),
        None,
        "trailing-spaces fix bails when the buffer does not parse"
    );
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

#[test]
fn apply_safe_fixes_runs_quoted_strings_fix() {
    let cfg = config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );

    let fixed = apply_safe_fixes(
        "foo: \"bar\"\n",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );

    assert_eq!(fixed, "foo: bar\n");
}

#[test]
fn apply_safe_fixes_skips_quoted_strings_when_unfixable() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "foo: \"bar\"\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[fix]\nunfixable = [\"quoted-strings\"]\n\n[rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
    )
    .unwrap();

    let ctx = discover_config(std::slice::from_ref(&file), &Overrides::default())
        .expect("config discovers");

    let fixed = apply_safe_fixes("foo: \"bar\"\n", &ctx.config, &file, &ctx.base_dir);

    assert_eq!(fixed, "foo: \"bar\"\n");
}

#[test]
fn fix_config_allows_quoted_strings_when_listed_in_fixable() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "foo: bar\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[fix]\nfixable = [\"quoted-strings\"]\n\n[rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
    )
    .unwrap();

    let ctx = discover_config(std::slice::from_ref(&file), &Overrides::default())
        .expect("config discovers");
    assert!(ctx.config.fix().allows_rule("quoted-strings"));
}

#[test]
fn fix_config_disallows_quoted_strings_when_not_listed() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "foo: bar\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[fix]\nfixable = [\"comments\"]\n\n[rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
    )
    .unwrap();

    let ctx = discover_config(std::slice::from_ref(&file), &Overrides::default())
        .expect("config discovers");
    assert!(!ctx.config.fix().allows_rule("quoted-strings"));
}
