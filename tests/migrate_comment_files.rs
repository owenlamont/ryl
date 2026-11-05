use std::fs;
use std::path::Path;

use ryl::migrate::{
    MigrateOptions, MigrationSource, OutputMode, SourceCleanup, WriteMode,
    apply_directive_entries, migrate_with_directives,
};
use tempfile::tempdir;

use crate::common::cli::{run, ryl};

fn options(source: Option<MigrationSource>) -> MigrateOptions {
    MigrateOptions {
        from: source,
        project_root: None,
        user_config: None,
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::SummaryOnly,
        cleanup: SourceCleanup::Keep,
    }
}

fn cli(home: &Path) -> std::process::Command {
    let mut command = ryl(home);
    command
        .current_dir(home)
        .env_remove("YAMLLINT_FILE_ENCODING")
        .env_remove("YAMLLINT_CONFIG_FILE");
    command
}

#[test]
fn directive_cli_previews_then_writes_beside_existing_native_config() {
    let dir = tempdir().unwrap();
    let native = "[format]\n";
    fs::write(dir.path().join(".ryl.toml"), native).unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    let path = dir.path().join("app.yaml");
    let input = "before: 0\n# prettier-ignore\nheld:    [1,2]\nafter: 3\n";
    fs::write(&path, input).unwrap();
    let (code, stdout, stderr) = run(cli(dir.path()).args(["--migrate-directives"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("+# ryl format-ignore"), "{stdout}");
    assert_eq!(fs::read_to_string(&path).unwrap(), input);
    let (code, _, stderr) =
        run(cli(dir.path()).args(["--migrate-directives", "--migrate-write"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        input.replace("prettier-ignore", "ryl format-ignore")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(".ryl.toml")).unwrap(),
        native
    );
    assert_eq!(
        fs::read_to_string(dir.path().join(".prettierrc")).unwrap(),
        "{}"
    );
}

#[test]
fn directive_cli_rejects_config_only_flags_without_config_trigger() {
    let dir = tempdir().unwrap();
    for args in [
        vec!["--migrate-directives", "--migrate-stdout"],
        vec![
            "--migrate-directives",
            "--migrate-write",
            "--migrate-delete-old",
        ],
        vec!["--migrate-directives", "app.yaml"],
        vec!["--migrate-from", "prettier"],
    ] {
        let (code, _, _) = run(cli(dir.path()).args(args));
        assert_eq!(code, 2);
    }
}

#[test]
fn explicit_application_file_uses_nearest_static_source_config() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".prettierrc"), r#"{"requirePragma":true}"#).unwrap();
    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join(".prettierrc"), "{}").unwrap();
    let path = nested.join("app.yaml");
    let input = "# prettier-ignore\nheld:    [1,2]\n";
    fs::write(&path, input).unwrap();
    let result =
        migrate_with_directives(&options(Some(MigrationSource::Prettier)), &path)
            .unwrap();
    assert_eq!(result.files.len(), 1);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(fs::read_to_string(path).unwrap(), input);
}

#[test]
fn competing_ancestor_formatter_configs_require_source_selection() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join(".yamlfmt"), "formatter: {}\n").unwrap();
    fs::write(nested.join("app.yaml"), "# prettier-ignore\nheld: 1\n").unwrap();
    let error = migrate_with_directives(&options(None), &nested)
        .err()
        .unwrap();
    assert!(error.to_string().contains("--migrate-from"));
    assert_eq!(
        migrate_with_directives(&options(Some(MigrationSource::Prettier)), &nested)
            .unwrap()
            .files
            .len(),
        1
    );
}

#[test]
fn uncertain_prettier_context_retains_formatter_marker_but_converts_aliases() {
    for (config, name, input) in [
        (
            r#"{"requirePragma":true}"#,
            "app.yaml",
            "# prettier-ignore\nheld: 1\n# yamllint disable\nother: 2\n",
        ),
        (
            r#"{"checkIgnorePragma":true}"#,
            "app.yaml",
            "# prettier-ignore\nheld: 1\n# yamllint disable\nother: 2\n",
        ),
        (
            r#"{"overrides":[{"files":"*.{yaml,md}","options":{}}]}"#,
            "app.yaml",
            "# prettier-ignore\nheld: 1\n# yamllint disable\nother: 2\n",
        ),
        (
            r#"{"plugins":[]}"#,
            "app.yaml",
            "# prettier-ignore\nheld: 1\n# yamllint disable\nother: 2\n",
        ),
        (
            r#"{"embeddedLanguageFormatting":"off"}"#,
            "app.md",
            "~~~yaml\n# prettier-ignore\nheld: 1\n# yamllint disable\nother: 2\n~~~\n",
        ),
        (
            r#"{"parser":"json"}"#,
            "app.yaml",
            "# prettier-ignore\nheld: 1\n# yamllint disable\nother: 2\n",
        ),
        (
            r#"{"parser":"markdown"}"#,
            "app.yaml",
            "# prettier-ignore\nheld: 1\n# yamllint disable\nother: 2\n",
        ),
        (
            r#"{"parser":"yaml"}"#,
            "app.md",
            "~~~yaml\n# prettier-ignore\nheld: 1\n# yamllint disable\nother: 2\n~~~\n",
        ),
    ] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".prettierrc"), config).unwrap();
        let path = dir.path().join(name);
        fs::write(&path, input).unwrap();
        let mut options = options(Some(MigrationSource::Prettier));
        options.write_mode = WriteMode::Write;
        let result = migrate_with_directives(&options, &path).unwrap();
        assert!(!result.warnings.is_empty());
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            input.replace("# yamllint", "# ryl"),
            "{config}"
        );
    }
}

#[test]
fn dprint_static_custom_marker_converts_without_importing_config() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("dprint.json"), r#"{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm"],"yaml":{"ignoreCommentDirective":"skip-yaml"}}"#).unwrap();
    fs::write(
        dir.path().join("app.yaml"),
        "before: 0\n# skip-yaml\nheld:    [1,2]\n",
    )
    .unwrap();
    let result =
        migrate_with_directives(&options(Some(MigrationSource::Dprint)), dir.path())
            .unwrap();
    assert_eq!(result.files.len(), 1);
    assert!(
        result.files[0]
            .diff
            .as_ref()
            .unwrap()
            .contains("+# ryl format-ignore")
    );
    assert!(!dir.path().join(".ryl.toml").exists());
}

#[test]
fn pretty_yaml_comment_conversion_requires_the_audited_plugin() {
    for config in [
        r#"{"yaml":{}}"#,
        r#"{"plugins":["https://example.invalid/pretty_yaml-v0.6.0.wasm"],"yaml":{}}"#,
        r#"{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.4.0.wasm"],"yaml":{}}"#,
        r#"{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm@bad"],"yaml":{}}"#,
    ] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("dprint.json"), config).unwrap();
        fs::write(
            dir.path().join("app.yaml"),
            "before: 0\n# pretty-yaml-ignore\nheld:    [1,2]\n",
        )
        .unwrap();
        let result = migrate_with_directives(
            &options(Some(MigrationSource::Dprint)),
            dir.path(),
        )
        .unwrap();
        assert!(
            result.files.is_empty(),
            "unverified plugin converted a marker: {config}"
        );
        assert!(!result.warnings.is_empty(), "{config}");
    }
}

#[test]
fn current_pretty_yaml_npm_reference_imports_defaults_and_converts_comments() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("dprint.json"),
        r#"{"plugins":["npm:dprint-plugin-yaml@0.6.0"]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("app.yaml"),
        "before: 0\n# pretty-yaml-ignore\nheld:    [1,2]\n",
    )
    .unwrap();
    let mut config_options = options(Some(MigrationSource::Dprint));
    config_options.project_root = Some(dir.path().to_path_buf());
    let imported = ryl::migrate::migrate_configs(&config_options).unwrap();
    assert_eq!(
        imported.entries.len(),
        1,
        "current npm plugin reference must identify default YAML formatter settings"
    );
    let config =
        ryl::config::YamlLintConfig::from_toml_str(&imported.entries[0].toml).unwrap();
    assert!(config.format().targets().keep_quotes);
    let comments =
        migrate_with_directives(&options(Some(MigrationSource::Dprint)), dir.path())
            .unwrap();
    assert_eq!(comments.files.len(), 1);
}

#[test]
fn skipped_explicit_source_config_never_becomes_an_application_file() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("custom.yaml");
    let input = "# yamllint disable\n'shared-prettier-config'\n";
    fs::write(&source, input).unwrap();
    let (code, stdout, stderr) = run(cli(dir.path())
        .args([
            "--migrate-configs",
            "--migrate-directives",
            "--migrate-from",
            "prettier",
            "--migrate-write",
            "--migrate-root",
        ])
        .arg(&source));
    assert_eq!(code, 2, "{stdout}\n{stderr}");
    assert_eq!(
        fs::read_to_string(&source).unwrap(),
        input,
        "skipped source configuration must remain byte-for-byte intact"
    );
    assert!(!dir.path().join(".ryl.toml").exists());
}

#[test]
fn prettier_default_parser_must_support_the_markdown_filename() {
    for (name, configured, expected) in [
        ("app.qmd", false, 0),
        ("app.Rmd", false, 0),
        ("app.qmd", true, 1),
        ("app.md", false, 1),
        ("app.mdx", false, 1),
    ] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(".prettierrc"),
            if configured {
                r#"{"parser":"markdown"}"#
            } else {
                "{}"
            },
        )
        .unwrap();
        let path = dir.path().join(name);
        let input = "~~~yaml\nbefore: 0\n# prettier-ignore\nheld:    [1,2]\n~~~\n";
        fs::write(&path, input).unwrap();
        let result =
            migrate_with_directives(&options(Some(MigrationSource::Prettier)), &path)
                .unwrap();
        assert_eq!(
            result.files.len(),
            expected,
            "{name}, explicit parser: {configured}"
        );
        assert_eq!(fs::read_to_string(path).unwrap(), input);
    }
}

#[test]
fn config_files_and_hard_link_aliases_are_preserved() {
    let dir = tempdir().unwrap();
    let config = dir.path().join(".prettierrc.yaml");
    let input = "# yamllint disable\n{}\n";
    fs::write(&config, input).unwrap();
    fs::hard_link(&config, dir.path().join("alias.yaml")).unwrap();
    let mut options = options(Some(MigrationSource::Prettier));
    options.write_mode = WriteMode::Write;
    let result = migrate_with_directives(&options, dir.path()).unwrap();
    assert!(result.files.is_empty());
    assert_eq!(fs::read_to_string(config).unwrap(), input);
}

#[test]
fn directive_plan_refuses_stale_application_bytes_before_writing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("app.yaml");
    fs::write(&path, "# yamllint disable\nheld: 1\n").unwrap();
    let result =
        migrate_with_directives(&options(Some(MigrationSource::Yamllint)), &path)
            .unwrap();
    fs::write(&path, "changed: true\n").unwrap();
    let error = apply_directive_entries(&result.files).unwrap_err();
    assert!(error.to_string().contains("changed"));
    assert_eq!(fs::read_to_string(path).unwrap(), "changed: true\n");
}

#[test]
fn combined_application_write_failure_keeps_source_config_after_target_write() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".yamllint");
    fs::write(&source, "rules: {truthy: enable}\n").unwrap();
    let path = dir.path().join("app.yaml");
    fs::write(&path, "# yamllint disable\nheld: yes\n").unwrap();
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut readonly = original_permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&path, readonly).unwrap();
    if fs::OpenOptions::new().write(true).open(&path).is_ok() {
        fs::set_permissions(&path, original_permissions).unwrap();
        return;
    }
    let (code, _, stderr) = run(cli(dir.path()).args([
        "--migrate-configs",
        "--migrate-directives",
        "--migrate-from",
        "yamllint",
        "--migrate-write",
        "--migrate-delete-old",
    ]));
    fs::set_permissions(&path, original_permissions).unwrap();
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("cannot write migrated comments"),
        "{stderr}"
    );
    assert!(
        dir.path().join(".ryl.toml").exists(),
        "config target must have been written before application failure"
    );
    assert!(
        source.exists(),
        "source cleanup must wait for successful application writes"
    );
}

#[test]
fn directive_only_missing_root_reports_configuration_error() {
    let dir = tempdir().unwrap();
    let error = migrate_with_directives(&options(None), &dir.path().join("missing"))
        .err()
        .unwrap();
    assert!(error.to_string().contains("migrate root does not exist"));
}

#[test]
fn combined_prettier_migration_omits_separate_directive_conversion_note() {
    for directives in [false, true] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
        let path = dir.path().join("app.yaml");
        let input = "before: 0\n# prettier-ignore\nheld:    [1,2]\n";
        fs::write(&path, input).unwrap();
        let mut command = cli(dir.path());
        command.args(["--migrate-configs", "--migrate-write"]);
        if directives {
            command.arg("--migrate-directives");
        }
        let (code, stdout, stderr) = run(&mut command);
        assert_eq!(code, 0, "{stdout}\n{stderr}");
        let toml = fs::read_to_string(dir.path().join(".ryl.toml")).unwrap();
        for output in [&stderr, &toml] {
            assert_eq!(
                output.contains(
                    "convert them with ryl --migrate-directives --migrate-from prettier"
                ),
                !directives,
                "{output}"
            );
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            if directives {
                input.replace("prettier-ignore", "ryl format-ignore")
            } else {
                input.to_owned()
            }
        );
    }
}

#[test]
fn config_import_only_leaves_comments_while_combined_conversion_is_opt_in() {
    for directives in [false, true] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        fs::write(&source, "formatter: {}\n").unwrap();
        let path = dir.path().join("app.yaml");
        let input =
            "# !yamlfmt!:ignore\n# yamllint disable rule:truthy\nheld:    [1,2]\n";
        fs::write(&path, input).unwrap();
        let mut command = cli(dir.path());
        command.args([
            "--migrate-configs",
            "--migrate-from",
            "yamlfmt",
            "--migrate-write",
            "--migrate-delete-old",
        ]);
        if directives {
            command.arg("--migrate-directives");
        }
        let (code, stdout, stderr) = run(&mut command);
        assert_eq!(code, 0, "{stdout}\n{stderr}");
        assert!(dir.path().join(".ryl.toml").exists());
        assert!(!source.exists());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            if directives {
                input
                    .replace("!yamlfmt!:ignore", "ryl format-disable")
                    .replace("yamllint", "ryl")
            } else {
                input.to_owned()
            }
        );
        let (formatted, _, stderr) =
            run(cli(dir.path()).args(["format", "--check"]).arg(&path));
        assert_eq!(formatted, if directives { 0 } else { 1 }, "{stderr}");
    }
}

#[test]
fn canonical_unicode_encodings_keep_boms_and_non_directive_bytes() {
    let dir = tempdir().unwrap();
    let input = "# yamllint disable\r\ncafé: 🦀\r\n";
    let expected = input.replace("yamllint", "ryl");
    for big in [false, true] {
        let encode = |text: &str| crate::common::encoding::encoded(text, 2, !big, true);
        let path = dir.path().join("app.yaml");
        let original = encode(input);
        fs::write(&path, &original).unwrap();
        let result =
            migrate_with_directives(&options(Some(MigrationSource::Yamllint)), &path)
                .unwrap();
        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].diff.is_none());
        assert_eq!(fs::read(&path).unwrap(), original);
        apply_directive_entries(&result.files).unwrap();
        assert_eq!(fs::read(&path).unwrap(), encode(&expected));
    }
    let path = dir.path().join("bom.yaml");
    let bytes = [b"\xef\xbb\xbf".as_slice(), input.as_bytes()].concat();
    fs::write(&path, bytes).unwrap();
    let mut options = options(Some(MigrationSource::Yamllint));
    options.write_mode = WriteMode::Write;
    migrate_with_directives(&options, &path).unwrap();
    assert_eq!(
        fs::read(path).unwrap(),
        [b"\xef\xbb\xbf".as_slice(), expected.as_bytes()].concat()
    );
}

#[test]
fn noncanonical_custom_encoding_is_reported_and_retained() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("app.yaml");
    let input = b"# yamllint disable\nvalue: '\x87\x90'\n";
    fs::write(&path, input).unwrap();
    let (code, stdout, stderr) = run(cli(dir.path())
        .env("YAMLLINT_FILE_ENCODING", "shift_jis")
        .args([
            "--migrate-directives",
            "--migrate-from",
            "yamllint",
            "--migrate-write",
        ]));
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stderr.contains("encoding does not round-trip exactly"),
        "{stderr}"
    );
    assert_eq!(fs::read(path).unwrap(), input);
}

#[test]
fn unknown_source_reports_actual_markers_in_yaml_and_markdown() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("app.yaml"), "# prettier-ignore\nheld: 1\n").unwrap();
    fs::write(
        dir.path().join("app.md"),
        "host\n\n~~~yaml\n# pretty-yaml-ignore\nheld: 1\n~~~\n",
    )
    .unwrap();
    fs::write(dir.path().join("plain.yaml"), "plain: 1\n").unwrap();
    fs::write(
        dir.path().join("app-yamlfmt.yaml"),
        "# !yamlfmt!:ignore\nheld: 1\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("ordinary.yaml"),
        "# ignore ordinary text\nheld: 1\n",
    )
    .unwrap();
    fs::write(dir.path().join("notes.txt"), "# prettier-ignore\n").unwrap();
    let result = migrate_with_directives(&options(None), dir.path()).unwrap();
    assert!(result.files.is_empty());
    assert_eq!(result.warnings.len(), 3, "{:?}", result.warnings);
    assert!(
        result
            .warnings
            .iter()
            .all(|warning| warning.contains("--migrate-from"))
    );
}

#[cfg(unix)]
#[test]
fn final_symlink_is_skipped_and_stale_symlink_plan_is_rejected() {
    let dir = tempdir().unwrap();
    let real = dir.path().join("real.yaml");
    let input = "# yamllint disable\nheld: 1\n";
    fs::write(&real, input).unwrap();
    let alias = dir.path().join("alias.yaml");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let skipped =
        migrate_with_directives(&options(Some(MigrationSource::Yamllint)), &alias)
            .unwrap();
    assert!(skipped.files.is_empty());
    assert!(
        skipped
            .warnings
            .iter()
            .any(|warning| warning.contains("symlink"))
    );
    let plan =
        migrate_with_directives(&options(Some(MigrationSource::Yamllint)), &real)
            .unwrap();
    fs::remove_file(&real).unwrap();
    std::os::unix::fs::symlink(&alias, &real).unwrap();
    assert!(
        apply_directive_entries(&plan.files)
            .unwrap_err()
            .to_string()
            .contains("symlinked")
    );
}

#[test]
fn unsupported_static_contexts_retain_formatter_markers() {
    for (name, config, source, input) in [
        (
            ".prettierrc.js",
            "throw new Error('must never execute');",
            MigrationSource::Prettier,
            "# prettier-ignore\nheld: 1\n",
        ),
        (
            "dprint.json",
            r#"{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm"],"extends":"https://example.invalid/base.json","yaml":{}}"#,
            MigrationSource::Dprint,
            "# pretty-yaml-ignore\nheld: 1\n",
        ),
        (
            "dprint.json",
            r#"{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm"],"yaml":{"ignoreCommentDirective":""}}"#,
            MigrationSource::Dprint,
            "#\nheld: 1\n",
        ),
        (
            "dprint.json",
            r#"{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm"],"yaml":{"ignoreCommentDirective":"custom "}}"#,
            MigrationSource::Dprint,
            "# custom \nheld: 1\n",
        ),
        (
            "dprint.json",
            r#"{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm"],"yaml":{"associations":["**/*.json"]}}"#,
            MigrationSource::Dprint,
            "# pretty-yaml-ignore\nheld: 1\n",
        ),
    ] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(name), config).unwrap();
        let path = dir.path().join("app.yaml");
        fs::write(&path, input).unwrap();
        let result = migrate_with_directives(&options(Some(source)), &path).unwrap();
        assert!(result.files.is_empty(), "{config}");
        assert!(!result.warnings.is_empty(), "{config}");
    }
}

#[test]
fn audited_pretty_yaml_integrity_suffix_keeps_known_scope() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("dprint.json"), format!(r#"{{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm@{}"],"yaml":{{}}}}"#, "40a2fdda7040317eb1b23520f3a00769a5571eedb049c4ca9175c1b9eeba01ae")).unwrap();
    fs::write(
        dir.path().join("app.yaml"),
        "# pretty-yaml-ignore\nheld: 1\n",
    )
    .unwrap();
    let result =
        migrate_with_directives(&options(Some(MigrationSource::Dprint)), dir.path())
            .unwrap();
    assert_eq!(result.files.len(), 1);
}

#[test]
fn directive_cli_reports_static_parse_and_option_errors() {
    for config in ["{broken", r#"{"tabWidth":"two"}"#] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".prettierrc"), config).unwrap();
        fs::write(dir.path().join("app.yaml"), "# prettier-ignore\nheld: 1\n").unwrap();
        let (code, _, stderr) = run(cli(dir.path()).args([
            "--migrate-directives",
            "--migrate-from",
            "prettier",
        ]));
        assert_eq!(code, 2, "{stderr}");
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn bad_encoding_and_markdown_carriage_returns_are_reported_without_edits() {
    for (name, bytes, message) in [
        ("bad.yaml", b"\xff\xff".as_slice(), "comments retained"),
        (
            "bad.md",
            b"~~~yaml\r# yamllint disable\rheld: 1\r~~~\r".as_slice(),
            "carriage return",
        ),
    ] {
        let dir = tempdir().unwrap();
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        let result =
            migrate_with_directives(&options(Some(MigrationSource::Yamllint)), &path)
                .unwrap();
        assert!(result.files.is_empty());
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains(message)),
            "{:?}",
            result.warnings
        );
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn directive_cli_previews_unrenderable_tail_and_reports_empty_scope() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("app.yaml");
    let input = "# yamllint disable\rheld: 1\r";
    fs::write(&path, input).unwrap();
    let (code, stdout, stderr) = run(cli(dir.path()).args([
        "--migrate-directives",
        "--migrate-from",
        "yamllint",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("Would migrate comments"));
    assert!(!stdout.contains("--- app.yaml"));
    assert_eq!(fs::read_to_string(&path).unwrap(), input);
    fs::write(&path, "held: 1\n").unwrap();
    let (code, stdout, stderr) = run(cli(dir.path()).args([
        "--migrate-directives",
        "--migrate-from",
        "yamllint",
    ]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("No source comments migrated"));
}

#[test]
fn directive_plan_reports_removed_application_file_cause() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("app.yaml");
    fs::write(&path, "# yamllint disable\nheld: 1\n").unwrap();
    let plan =
        migrate_with_directives(&options(Some(MigrationSource::Yamllint)), &path)
            .unwrap();
    fs::remove_file(&path).unwrap();
    let error = apply_directive_entries(&plan.files).unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn combined_migration_preserves_sources_on_config_planning_and_write_errors() {
    for failure in ["invalid", "backup", "target"] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamllint");
        fs::write(
            &source,
            if failure == "invalid" {
                "rules: [truthy]\n"
            } else {
                "rules: {truthy: enable}\n"
            },
        )
        .unwrap();
        let path = dir.path().join("app.yaml");
        let input = "# yamllint disable\nheld: yes\n";
        fs::write(&path, input).unwrap();
        let mut command = cli(dir.path());
        command.args([
            "--migrate-configs",
            "--migrate-directives",
            "--migrate-from",
            "yamllint",
            "--migrate-write",
        ]);
        if failure == "backup" {
            fs::write(dir.path().join(".yamllint.bak"), "backup").unwrap();
            command.args(["--migrate-rename-old", ".bak"]);
        } else if failure == "target" {
            fs::create_dir(dir.path().join(".ryl.toml")).unwrap();
        }
        let (code, _, stderr) = run(&mut command);
        assert_eq!(code, 2, "{failure}: {stderr}");
        assert!(source.exists());
        assert_eq!(fs::read_to_string(path).unwrap(), input);
    }
}

#[cfg(unix)]
#[test]
fn directive_cli_reports_unreadable_file_directory_and_context() {
    use std::os::unix::fs::PermissionsExt as _;
    for stage in ["file", "walk", "context"] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.yaml");
        let input = "# yamllint disable\nheld: 1\n";
        fs::write(&path, input).unwrap();
        let restricted = if stage == "file" {
            path.as_path()
        } else {
            dir.path()
        };
        let original = fs::metadata(restricted).unwrap().permissions();
        fs::set_permissions(
            restricted,
            fs::Permissions::from_mode(if stage == "file" { 0 } else { 0o111 }),
        )
        .unwrap();
        let privileged = if stage == "file" {
            fs::read(&path).is_ok()
        } else {
            fs::read_dir(dir.path()).is_ok()
        };
        if privileged {
            fs::set_permissions(restricted, original).unwrap();
            continue;
        }
        let mut command = cli(dir.path());
        command.args([
            "--migrate-directives",
            "--migrate-from",
            if stage == "context" {
                "prettier"
            } else {
                "yamllint"
            },
        ]);
        if stage != "walk" {
            command.arg("--migrate-root").arg(&path);
        }
        let (code, _, stderr) = run(&mut command);
        fs::set_permissions(restricted, original).unwrap();
        assert_eq!(code, 2, "{stage}: {stderr}");
        assert!(stderr.contains("cannot"), "{stage}: {stderr}");
        assert_eq!(fs::read_to_string(&path).unwrap(), input);
    }
}

#[cfg(unix)]
#[test]
fn directive_plan_reports_lost_read_permission_before_write() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempdir().unwrap();
    let path = dir.path().join("app.yaml");
    fs::write(&path, "# yamllint disable\nheld: 1\n").unwrap();
    let plan =
        migrate_with_directives(&options(Some(MigrationSource::Yamllint)), &path)
            .unwrap();
    let original = fs::metadata(&path).unwrap().permissions();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
    if fs::read(&path).is_ok() {
        fs::set_permissions(&path, original).unwrap();
        return;
    }
    let result = apply_directive_entries(&plan.files);
    fs::set_permissions(&path, original).unwrap();
    assert!(std::error::Error::source(&result.unwrap_err()).is_some());
}

#[cfg(unix)]
#[test]
fn combined_cleanup_failure_keeps_successful_writes_and_reports_source() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempdir().unwrap();
    let legacy = dir.path().join("legacy");
    let apps = dir.path().join("apps");
    fs::create_dir(&legacy).unwrap();
    fs::create_dir(&apps).unwrap();
    let source = legacy.join("config");
    fs::write(&source, "rules: {truthy: enable}\n").unwrap();
    let path = apps.join("app.yaml");
    fs::write(&path, "# yamllint disable\nheld: yes\n").unwrap();
    let original = fs::metadata(&legacy).unwrap().permissions();
    fs::set_permissions(&legacy, fs::Permissions::from_mode(0o500)).unwrap();
    let probe = legacy.join("probe");
    if fs::write(&probe, "").is_ok() {
        fs::remove_file(probe).unwrap();
        fs::set_permissions(&legacy, original).unwrap();
        return;
    }
    let target = dir.path().join("native/ryl.toml");
    let mut options = options(Some(MigrationSource::Yamllint));
    options.user_config = Some(ryl::migrate::UserConfigMigration {
        source: source.clone(),
        target: target.clone(),
    });
    options.cleanup = SourceCleanup::Delete;
    options.write_mode = WriteMode::Write;
    let result = migrate_with_directives(&options, &apps);
    fs::set_permissions(&legacy, original).unwrap();
    let error = result.err().unwrap();
    assert!(
        error
            .to_string()
            .contains("failed to delete migrated source"),
        "{error}"
    );
    assert!(target.exists());
    assert!(source.exists());
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .starts_with("# ryl disable")
    );
}

#[test]
fn imported_prettier_quote_and_prose_policies_respect_converted_node_ignore() {
    for policy in ["preserve", "always", "never"] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(".prettierrc"),
            format!(r#"{{"singleQuote":true,"proseWrap":"{policy}","printWidth":24}}"#),
        )
        .unwrap();
        let held =
            "held:\n  quote: \"held\"\n  text: >\n    held first\n    held second\n";
        let input = format!(
            "before: \"before\"\n# prettier-ignore\n{held}after: \"after\"\ntext: >\n  outside\n  words fit\n"
        );
        let path = dir.path().join("app.yaml");
        fs::write(&path, &input).unwrap();
        let (code, _, stderr) = run(cli(dir.path()).args([
            "--migrate-configs",
            "--migrate-directives",
            "--migrate-from",
            "prettier",
            "--migrate-write",
        ]));
        assert_eq!(code, 0, "{policy}: {stderr}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            input.replace("prettier-ignore", "ryl format-ignore")
        );
        let (code, _, stderr) = run(cli(dir.path()).arg("format").arg(&path));
        assert_eq!(code, 0, "{policy}: {stderr}");
        let output = fs::read_to_string(&path).unwrap();
        assert!(
            output.contains(held),
            "ignored subtree changed under {policy}: {output}"
        );
        assert!(
            output.contains("before: 'before'\n"),
            "quote witness unchanged: {output}"
        );
        assert!(
            output.contains("after: 'after'\n"),
            "quote witness unchanged: {output}"
        );
        let prose = if policy == "preserve" {
            "text: >\n  outside\n  words fit\n"
        } else {
            "text: >\n  outside words fit\n"
        };
        assert!(output.ends_with(prose), "{policy}: {output}");
        let (code, _, stderr) =
            run(cli(dir.path()).args(["format", "--check"]).arg(&path));
        assert_eq!(code, 0, "{policy}: {stderr}");
    }
}

#[test]
fn yaml_and_unrelated_prettier_overrides_allow_yaml_comment_conversion() {
    for (override_body, converted) in [
        (
            r#"{"files":"*.yaml","options":{"proseWrap":"preserve"}}"#,
            1,
        ),
        (r#"{"files":"*.md","options":{"parser":"markdown"}}"#, 1),
        (r#"{"files":"*.{yaml,md}","options":{}}"#, 0),
        (r#"{"files":"*.yaml","options":{"parser":"yaml"}}"#, 0),
        (r#"{"files":"*.yaml","options":{"plugins":[]}}"#, 0),
        (
            r#"{"files":"*.yaml","excludeFiles":"other.yaml","options":{}}"#,
            0,
        ),
        (r#"{"files":"*.yaml","options":{"requirePragma":false}}"#, 0),
    ] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(".prettierrc"),
            format!(r#"{{"overrides":[{override_body}]}}"#),
        )
        .unwrap();
        fs::write(
            dir.path().join("app.yaml"),
            "before: 0\n# prettier-ignore\nheld:    [1,2]\n",
        )
        .unwrap();
        let result = migrate_with_directives(
            &options(Some(MigrationSource::Prettier)),
            dir.path(),
        )
        .unwrap();
        assert_eq!(result.counts.converted, converted);
        assert_eq!(result.counts.retained, 1 - converted);
        assert_eq!(result.files.len(), converted);
    }
}

#[test]
fn migration_stdout_is_loadable_toml_and_exports_beside_an_existing_target() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    let target = dir.path().join(".ryl.toml");
    let native = "[lint.rules]\nanchors = 'enable'\n";
    fs::write(&target, native).unwrap();
    fs::write(
        dir.path().join("app.yaml"),
        "before: 0\n# prettier-ignore\nheld:    [1,2]\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(cli(dir.path()).args([
        "--migrate-configs",
        "--migrate-directives",
        "--migrate-stdout",
    ]));
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stderr.contains(" -> "));
    assert!(stdout.contains("not merged"));
    let redirected = dir.path().join("export.toml");
    fs::write(&redirected, stdout).unwrap();
    assert!(
        ryl::config::YamlLintConfig::from_toml_str(
            &fs::read_to_string(redirected).unwrap()
        )
        .is_ok()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), native);
    let (code, _, stderr) =
        run(cli(dir.path()).args(["--migrate-configs", "--migrate-write"]));
    assert_eq!(code, 2, "{stderr}");
    assert_eq!(fs::read_to_string(target).unwrap(), native);
}

#[test]
fn retained_directive_reruns_report_totals_and_strict_warnings() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    fs::write(dir.path().join("app.yaml"), "before: 0\n# prettier-ignore\nheld: 1\n# prettier-ignore # reason\nretained: 2\n").unwrap();
    for (converted, strict, expected) in [(1, false, 0), (0, false, 0), (0, true, 2)] {
        let mut command = cli(dir.path());
        command.args(["--migrate-directives", "--migrate-write"]);
        if strict {
            command.arg("--strict");
        }
        let (code, stdout, stderr) = run(&mut command);
        assert_eq!(code, expected, "{stdout}\n{stderr}");
        assert!(
            stderr.contains(&format!(
                "{converted} converted, 1 retained, 0 skipped files"
            )),
            "{stderr}"
        );
        assert!(!stdout.contains("No source comments migrated"));
    }
}

#[test]
fn skipped_sources_exit_two_but_clean_migration_reruns_succeed() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".prettierrc.js"), "module.exports = {};").unwrap();
    let (code, _, stderr) =
        run(cli(dir.path()).args(["--migrate-configs", "--migrate-write"]));
    assert_eq!(code, 2, "{stderr}");
    fs::remove_file(dir.path().join(".prettierrc.js")).unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    let (code, _, stderr) =
        run(cli(dir.path()).args(["--migrate-configs", "--migrate-write"]));
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) =
        run(cli(dir.path()).args(["--migrate-configs", "--migrate-write", "--strict"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn pretty_yaml_five_series_converts_the_same_node_ignore_marker() {
    for plugin in [
        "https://plugins.dprint.dev/g-plane/pretty_yaml-v0.5.0.wasm",
        "https://plugins.dprint.dev/g-plane/pretty_yaml-v0.5.1.wasm",
        "npm:dprint-plugin-yaml@0.5.1",
    ] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("dprint.json"),
            format!(r#"{{"plugins":["{plugin}"]}}"#),
        )
        .unwrap();
        fs::write(
            dir.path().join("app.yaml"),
            "before: 0\n# pretty-yaml-ignore\nheld:    [1,2]\n",
        )
        .unwrap();
        let result = migrate_with_directives(
            &options(Some(MigrationSource::Dprint)),
            dir.path(),
        )
        .unwrap();
        assert_eq!(result.counts.converted, 1, "{plugin}");
        assert_eq!(result.counts.retained, 0, "{plugin}");
    }
}

#[test]
fn clean_legacy_migration_rerun_does_not_repeat_conversion_warnings() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".yamllint"), "rules: {}\n").unwrap();
    let (code, _, stderr) =
        run(cli(dir.path()).args(["--migrate-configs", "--migrate-write"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.contains("enables no rules"));
    let (code, _, stderr) =
        run(cli(dir.path()).args(["--migrate-configs", "--migrate-write", "--strict"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn directive_only_stdout_explains_the_default_preview() {
    let dir = tempdir().unwrap();
    let (code, stdout, stderr) =
        run(cli(dir.path()).args(["--migrate-directives", "--migrate-stdout"]));
    assert_eq!(code, 2, "{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert_eq!(
        stderr.trim(),
        "error: `--migrate-stdout` applies to config migration; `--migrate-directives` previews changes by default"
    );
}
