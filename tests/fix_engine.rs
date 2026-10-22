use std::fs;
use std::path::Path;
use std::sync::Arc;

use ryl::config::{Overrides, SourceKind, YamlLintConfig, discover_config};
use ryl::fix::{FixStats, Rewrite, apply_safe_fixes, rewrite_files};
use tempfile::tempdir;

fn config(yaml: &str) -> YamlLintConfig {
    YamlLintConfig::from_yaml_str(yaml).expect("config parses")
}

fn rewrite_one_file(path: &Path, cfg: YamlLintConfig, base_dir: &Path) -> FixStats {
    rewrite_files(
        &[(
            path.to_path_buf(),
            base_dir.to_path_buf(),
            Arc::new(cfg),
            SourceKind::Yaml,
        )],
        Rewrite::Fix,
    )
}

#[test]
fn apply_safe_fixes_in_place_reports_no_change() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value\n").unwrap();
    let cfg = config(
        "rules:\n  comments: enable\n  new-lines: enable\n  new-line-at-end-of-file: enable\n",
    );

    let stats = rewrite_one_file(&file, cfg, dir.path());

    assert_eq!(stats.changed_files, 0);
    assert!(stats.notices.is_empty());
    assert!(stats.errors.is_empty());
    assert_eq!(fs::read_to_string(&file).unwrap(), "key: value\n");
}

#[cfg(unix)]
#[test]
fn apply_safe_fixes_in_place_refuses_symlink() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let target = dir.path().join("target.yaml");
    let link = dir.path().join("link.yaml");
    fs::write(&target, "key:  value\n").unwrap();
    symlink(&target, &link).unwrap();
    let cfg = config("rules:\n  colons: enable\n");

    let stats = rewrite_one_file(&link, cfg, dir.path());

    assert_eq!(stats.changed_files, 0);
    assert!(
        String::from_utf8_lossy(&stats.notices)
            .contains("refusing to follow a symlink")
    );
    assert!(stats.errors.is_empty());
    assert_eq!(fs::read_to_string(&target).unwrap(), "key:  value\n");
}

#[test]
fn apply_safe_fixes_in_place_writes_changes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    let cfg = config("rules:\n  comments: enable\n  new-line-at-end-of-file: enable\n");

    let stats = rewrite_one_file(&file, cfg, dir.path());

    assert_eq!(stats.changed_files, 1);
    assert!(stats.notices.is_empty());
    assert!(stats.errors.is_empty());
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

    let stats = rewrite_one_file(&file, cfg, dir.path());

    assert_eq!(stats.changed_files, 0, "an unparsable file is not changed");
    assert!(stats.errors.is_empty());
    let notices = String::from_utf8(stats.notices).unwrap();
    assert!(
        notices.lines().count() == 1
            && notices.starts_with(&format!("{}:1:", file.display()))
            && notices.contains("skipped by --fix")
            && notices.contains("unknown anchor"),
        "one skip notice carries the parse error: {notices}"
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

    let stats = rewrite_one_file(&file, cfg, dir.path());

    assert_eq!(stats.changed_files, 0);
    assert_eq!(stats.errors.len(), 1);
    assert_eq!(stats.errors[0].0, 0);
    assert!(stats.errors[0].1.contains("failed to read"));
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

    let stats = rewrite_one_file(&file, cfg, dir.path());

    assert_eq!(stats.changed_files, 0);
    assert_eq!(stats.errors.len(), 1);
    assert_eq!(stats.errors[0].0, 0);
    assert!(stats.errors[0].1.contains("failed to write fixed file"));
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
    let cfg = Arc::new(config(
        "rules:\n  comments: enable\n  new-line-at-end-of-file: enable\n",
    ));
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

    let stats = rewrite_files(&files, Rewrite::Fix);

    assert_eq!(
        fs::read_to_string(&first).unwrap(),
        "key: value  # comment\n"
    );
    assert_eq!(fs::read_to_string(&second).unwrap(), "alpha: beta\n");
    assert_eq!(stats.changed_files, 2);
    assert!(stats.errors.is_empty());
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
        "[lint]\nunfixable = [\"quoted-strings\"]\n\n[lint.rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
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
        "[lint]\nfixable = [\"quoted-strings\"]\n\n[lint.rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
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
        "[lint]\nfixable = [\"comments\"]\n\n[lint.rules.quoted-strings]\nquote-type = 'single'\nrequired = 'only-when-needed'\n",
    )
    .unwrap();

    let ctx = discover_config(std::slice::from_ref(&file), &Overrides::default())
        .expect("config discovers");
    assert!(!ctx.config.fix().allows_rule("quoted-strings"));
}

#[test]
fn document_end_and_trailing_spaces_settle_together() {
    let cfg =
        config("rules:\n  document-end: {present: true}\n  trailing-spaces: enable\n");
    let fixed = apply_safe_fixes(
        "a: |-\n  x\n  ",
        &cfg,
        std::path::Path::new("input.yaml"),
        std::path::Path::new("."),
    );
    assert_eq!(fixed, "a: |-\n  x\n...\n");
}

#[test]
fn an_unfixable_new_lines_still_sets_the_appended_final_newline() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        dir.path().join(".ryl.toml"),
        "[lint]\nunfixable = [\"new-lines\"]\n\n[lint.rules]\nnew-line-at-end-of-file = 'enable'\n\n[lint.rules.new-lines]\ntype = 'dos'\n",
    )
    .unwrap();
    let ctx = discover_config(std::slice::from_ref(&file), &Overrides::default())
        .expect("config discovers");

    let fixed = apply_safe_fixes("a: 1\nb: 2", &ctx.config, &file, &ctx.base_dir);

    assert_eq!(fixed, "a: 1\nb: 2\r\n");
}

fn fix_toml(toml: &str, input: &str) -> String {
    let cfg = YamlLintConfig::from_toml_str(toml).expect("test config parses");
    apply_safe_fixes(input, &cfg, Path::new("input.yaml"), Path::new("."))
}

#[test]
fn ambiguous_block_scalars_still_get_verified_key_ordering() {
    assert_eq!(
        fix_toml(
            "[lint.rules]\nkey-ordering = \"enable\"\n",
            "b: 1\naé: 1\nz: |1\n \n\n...\n"
        ),
        "aé: 1\nb: 1\nz: |1\n \n\n...\n"
    );
}

#[test]
fn forbidden_flow_mappings_keep_the_comma_spacing_fix() {
    assert_eq!(
        fix_toml(
            "[lint.rules]\ncommas = \"enable\"\n[lint.rules.braces]\nforbid = \"non-empty\"\n",
            "café: {b: 1,}\n"
        ),
        "café: {b: 1, }\n"
    );
}

#[test]
fn markers_bracket_an_unterminated_document() {
    assert_eq!(
        fix_toml(
            "[lint.rules]\ndocument-start = \"enable\"\n[lint.rules.document-end]\npresent = true\n",
            "café: 1"
        ),
        "---\ncafé: 1\n...\n"
    );
}

#[test]
fn guarded_magic_lines_move_below_the_marker_once_spaced() {
    assert_eq!(
        fix_toml(
            "[lint.rules]\ndocument-start = \"enable\"\n[lint.rules.comments]\nignore-shebangs = false\n",
            "#cloud-config\ncafé: 1  # ryl disable-line rule:colons\n"
        ),
        "---\n# cloud-config\ncafé: 1  # ryl disable-line rule:colons\n"
    );
}

#[test]
fn closing_brackets_keep_a_comma_spacing_they_allow() {
    assert_eq!(
        fix_toml(
            "[lint.rules.commas]\nmin-spaces-after = 2\nmax-spaces-after = 2\n[lint.rules.brackets]\nmax-spaces-inside = 1\n",
            "café: [b, ]\n"
        ),
        "café: [b, ]\n"
    );
}

#[test]
fn keep_scalar_newlines_precede_the_document_end() {
    assert_eq!(
        fix_toml(
            "[lint.rules]\ntrailing-spaces = \"enable\"\n[lint.rules.document-end]\npresent = true\n",
            "café: |+\n  x\n  "
        ),
        "café: |+\n  x\n\n...\n"
    );
}
