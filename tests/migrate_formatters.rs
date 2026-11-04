use std::fs;
use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::config_schema::{
    ExpandTarget, LineEndingTarget, MarkerTarget, PropertyOrderTarget, ProseWrap,
    QuoteStyleTarget,
};
use ryl::migrate::{
    MigrateOptions, MigrateResult, MigrationSource, OutputMode, SourceCleanup,
    WriteMode, migrate_configs,
};
use tempfile::tempdir;

use crate::common::cli::{run, ryl};

const DPRINT: &str = r#"{"plugins":["https://plugins.dprint.dev/g-plane/pretty_yaml-v0.6.0.wasm"],"yaml":{}}"#;

#[test]
fn yamlfmt_engine_dimension_defaults_and_array_indent_gaps_are_imported() {
    for (indent, width, expected_indent, expected_width, array_gap) in [
        (0, 8, 4, 80, false),
        (1, 4, 2, 80, true),
        (2, 5, 2, 5, false),
        (8, 16, 8, 80, false),
        (8, 17, 8, 17, false),
        (9, 19, 9, 19, false),
        (10, 4, 2, 80, true),
        (300, 81, 2, 81, true),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        fs::write(
            &source,
            format!("formatter: {{indent: {indent}, max_line_length: {width}}}\n"),
        )
        .unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
        let config = converted(&result);
        assert_eq!(
            ryl::format::indent_width(&config),
            expected_indent,
            "{indent}"
        );
        assert_eq!(
            ryl::format::line_length(&config),
            expected_width,
            "{indent}, {width}"
        );
        assert_eq!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("array_indent")),
            array_gap,
            "{result:?}"
        );
    }
    let dir = tempdir().unwrap();
    let source = dir.path().join(".yamlfmt");
    fs::write(&source, "formatter: {indent: -1}").unwrap();
    let mut opts = options(&source, Some(MigrationSource::Yamlfmt));
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    assert!(
        migrate_configs(&opts)
            .unwrap_err()
            .contains("must not be negative")
    );
    assert!(source.exists());
    assert!(!dir.path().join(".ryl.toml").exists());
}

#[test]
fn bare_formatter_filenames_match_dot_prefixed_paths_without_panicking() {
    for (tool, name, body) in [
        ("prettier", ".prettierrc.json", "{}"),
        ("yamlfmt", ".yamlfmt", "{}"),
        ("dprint", "dprint.json", DPRINT),
        (
            "yamlfix",
            "pyproject.toml",
            "[tool.yamlfix]\nline_length=90\n",
        ),
    ] {
        for prefix in ["", "./"] {
            for lint in [false, true] {
                let dir = tempdir().unwrap();
                fs::write(dir.path().join(name), body).unwrap();
                if lint {
                    fs::write(
                        dir.path().join(".yamllint"),
                        "rules: {truthy: enable}\n",
                    )
                    .unwrap();
                }
                let (code, _, stderr) =
                    run(ryl(dir.path()).current_dir(dir.path()).args([
                        "--migrate-configs",
                        "--migrate-root",
                        &format!("{prefix}{name}"),
                        "--migrate-from",
                        tool,
                        "--migrate-write",
                    ]));
                assert_eq!(code, 0, "{tool}, {prefix}, lint={lint}: {stderr}");
                let target = fs::read_to_string(dir.path().join(".ryl.toml")).unwrap();
                assert_eq!(target.contains("truthy"), lint, "{target}");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn symlink_parent_segments_do_not_bypass_physical_ancestor_lint_or_native_policy() {
    use std::os::unix::fs::symlink;

    for native in [false, true] {
        let dir = tempdir().unwrap();
        let actual = dir.path().join("real/actual");
        let logical = dir.path().join("logical");
        fs::create_dir_all(actual.join("nested")).unwrap();
        fs::create_dir(&logical).unwrap();
        symlink(actual.join("nested"), logical.join("link")).unwrap();
        fs::write(
            actual.parent().unwrap().join(if native {
                ".ryl.toml"
            } else {
                ".yamllint"
            }),
            if native {
                "[lint.rules]\ntruthy='enable'\n"
            } else {
                "rules: {truthy: enable}\n"
            },
        )
        .unwrap();
        fs::write(actual.join(".yamlfmt"), "{}").unwrap();
        let input = actual.join("input.yaml");
        fs::write(&input, "flag: yes\n").unwrap();
        let (before, _, stderr) = run(ryl(dir.path()).arg("check").arg(&input));
        assert_eq!(before, 1, "{stderr}");
        let mut opts =
            options(&logical.join("link/.."), Some(MigrationSource::Yamlfmt));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        let result = migrate_configs(&opts).unwrap();
        assert!(result.entries.is_empty(), "{result:?}");
        assert!(!actual.join(".ryl.toml").exists());
        assert!(actual.join(".yamlfmt").exists());
        let (after, _, stderr) = run(ryl(dir.path()).arg("check").arg(&input));
        assert_eq!(after, 1, "{stderr}");
    }
}

#[cfg(unix)]
#[test]
fn yamlfix_ancestor_lookup_and_cleanup_follow_physical_directory_context() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let actual = dir.path().join("real/actual");
    let logical = dir.path().join("logical");
    fs::create_dir_all(actual.join("nested")).unwrap();
    fs::create_dir(&logical).unwrap();
    symlink(actual.join("nested"), logical.join("link")).unwrap();
    let ancestor = actual.parent().unwrap().join("yamlfix.toml");
    fs::write(&ancestor, "line_length=90\n").unwrap();
    fs::write(actual.join(".yamlfix.toml"), "indent_mapping=4\n").unwrap();
    let sentinel = logical.join("yamlfix.toml");
    fs::write(&sentinel, "line_length=120\n").unwrap();
    let mut opts = options(&logical.join("link/.."), Some(MigrationSource::Yamlfix));
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    let result = migrate_configs(&opts).unwrap();
    assert_eq!(ryl::format::line_length(&converted(&result)), 90);
    assert!(ancestor.exists());
    assert!(!actual.join(".yamlfix.toml").exists());
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "line_length=120\n");
}

#[test]
fn malformed_known_unmapped_options_never_write_targets_or_delete_sources() {
    let mut cases = Vec::new();
    for key in [
        "disallow_anchors",
        "drop_merge_tag",
        "trim_trailing_whitespace",
        "eof_newline",
        "strip_directives",
        "disable_alias_key_correction",
        "line_ending",
    ] {
        cases.push((
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            format!("formatter: {{{key}: []}}"),
        ));
    }
    for key in [
        "include",
        "exclude",
        "regex_exclude",
        "extensions",
        "match_type",
        "gitignore_path",
        "output_format",
        "doublestar",
        "gitignore_excludes",
        "continue_on_error",
    ] {
        cases.push((
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            format!("{key}: {{bad: value}}"),
        ));
    }
    for (key, value) in [
        ("requirePragma", "'yes'"),
        ("insertPragma", "[]"),
        ("checkIgnorePragma", "1"),
        ("plugins", "true"),
        ("plugins", "null"),
        ("plugins", "[1]"),
        ("overrides", "true"),
        ("overrides", "[{}]"),
        ("overrides", "[{files:true}]"),
        ("overrides", "[{files:'*.yaml',excludeFiles:true}]"),
    ] {
        cases.push((
            MigrationSource::Prettier,
            ".prettierrc.json5",
            format!("{{{key}:{value}}}"),
        ));
    }
    for key in ["plugins", "includes", "excludes", "extends", "incremental"] {
        cases.push((
            MigrationSource::Dprint,
            "dprint.json",
            format!(r#"{{"{key}":null,"yaml":{{}}}}"#),
        ));
    }
    for key in ["locked", "associations"] {
        cases.push((
            MigrationSource::Dprint,
            "dprint.json",
            format!(r#"{{"yaml":{{"{key}":null}}}}"#),
        ));
    }
    cases.push((
        MigrationSource::Yamlfix,
        "yamlfix.toml",
        "config_path=[]".to_owned(),
    ));
    for (tool, name, body) in cases {
        let dir = tempdir().unwrap();
        let source = dir.path().join(name);
        fs::write(&source, &body).unwrap();
        let mut opts = options(&source, Some(tool));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        assert!(migrate_configs(&opts).is_err(), "{tool:?}: {body}");
        assert_eq!(fs::read_to_string(source).unwrap(), body);
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn well_typed_unmapped_options_are_reported_without_running_source_behavior() {
    for (tool, name, bodies) in [
        (
            MigrationSource::Prettier,
            ".prettierrc.json5",
            vec![
                "{plugins:[],overrides:null,requirePragma:null,insertPragma:true,checkIgnorePragma:false}",
                "{plugins:[{},'never-executed'],overrides:[]}",
                "{overrides:false}",
                "{overrides:0}",
                "{overrides:[{files:'*.yaml',excludeFiles:null},{files:'*.yaml',excludeFiles:false},{files:'*.yaml',excludeFiles:0}]}",
                "{overrides:[{files:['*.yaml'],excludeFiles:['ignored.yaml'],options:{}}]}",
            ],
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            vec![
                "include: []\nexclude: ['*.other']\nregex_exclude: null\nextensions: ['yml']\nmatch_type: 'standard'\ngitignore_path: null\noutput_format: 'line'\ndoublestar: false\ngitignore_excludes: null\ncontinue_on_error: true\nformatter: {eof_newline: null, disallow_anchors: true, drop_merge_tag: false, trim_trailing_whitespace: true, strip_directives: false, disable_alias_key_correction: false, line_ending: lf}",
            ],
        ),
        (
            MigrationSource::Dprint,
            "dprint.json",
            vec![
                r#"{"yaml":{"locked":true,"associations":"*.yaml"},"plugins":[],"extends":[],"includes":[],"excludes":["*.other"],"incremental":false}"#,
                r#"{"yaml":{"locked":false,"associations":["*.yaml"]}}"#,
            ],
        ),
        (
            MigrationSource::Yamlfix,
            "yamlfix.toml",
            vec!["config_path='never-read'\n"],
        ),
    ] {
        for body in bodies {
            let dir = tempdir().unwrap();
            let source = dir.path().join(name);
            fs::write(&source, body).unwrap();
            let result = migrate_configs(&options(&source, Some(tool))).unwrap();
            assert_eq!(result.entries.len(), 1, "{tool:?}: {body}: {result:?}");
            assert!(!result.warnings.is_empty());
            assert_eq!(fs::read_to_string(source).unwrap(), body);
        }
    }
}

#[test]
fn prettier_integral_float_dimensions_match_javascript_integer_controls() {
    for (name, body) in [
        (
            ".prettierrc.json",
            "{\"tabWidth\":2.0,\"printWidth\":80.0,\"useTabs\":null}",
        ),
        (
            ".prettierrc.yaml",
            "tabWidth: 2.0\nprintWidth: 80.0\nuseTabs: null\n",
        ),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(name);
        fs::write(&source, body).unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Prettier)))
                .unwrap();
        assert_eq!(ryl::format::line_length(&converted(&result)), 80);
        assert_eq!(ryl::format::indent_width(&converted(&result)), 2);
        for (key, value) in [("printWidth", "80.5"), ("tabWidth", "\"2\"")] {
            fs::write(&source, format!("{{\"{key}\": {value}}}")).unwrap();
            assert!(
                migrate_configs(&options(&source, Some(MigrationSource::Prettier)))
                    .is_err()
            );
        }
    }
}

#[test]
fn yamlfmt_invalid_output_format_is_not_a_cleanup_candidate() {
    for value in ["\"garbage\"", "\"default\"", "\"line\"", "\"\"", "null"] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        let body = format!("output_format: {value}\n");
        fs::write(&source, &body).unwrap();
        let mut opts = options(dir.path(), Some(MigrationSource::Yamlfmt));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        let result = migrate_configs(&opts);
        if value == "\"garbage\"" {
            assert!(result.unwrap_err().contains("output_format"));
            assert_eq!(fs::read_to_string(source).unwrap(), body);
            assert!(!dir.path().join(".ryl.toml").exists());
        } else {
            assert_eq!(result.unwrap().entries.len(), 1);
            assert!(!source.exists());
            assert!(dir.path().join(".ryl.toml").exists());
        }
    }
}

#[test]
fn duplicate_yaml_options_are_rejected_before_any_write_or_cleanup() {
    for (tool, name, bodies) in [
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            vec![
                "formatter: {indent: 4, indent: 6}",
                "formatter: {}\nformatter: {indent: 6}",
                "formatter: {indent: 2}\nunknown: {'<<': 1, <<: 2}",
                "formatter:\n  indent: &value 4\n  indent: *value\n",
            ],
        ),
        (
            MigrationSource::Prettier,
            ".prettierrc.yaml",
            vec![
                "tabWidth: 4\ntabWidth: 6\n",
                "unknown: {nested: 1, nested: 2}",
                "'<<': {tabWidth: 4}\n<<: {tabWidth: 6}\n",
            ],
        ),
    ] {
        for body in bodies {
            let dir = tempdir().unwrap();
            let source = dir.path().join(name);
            fs::write(&source, body).unwrap();
            let mut opts = options(&source, Some(tool));
            opts.write_mode = WriteMode::Write;
            opts.cleanup = SourceCleanup::Delete;
            let error = migrate_configs(&opts).unwrap_err();
            assert!(
                error.contains("duplicate YAML configuration key"),
                "{error}"
            );
            assert_eq!(fs::read_to_string(source).unwrap(), body);
            assert!(!dir.path().join(".ryl.toml").exists());
        }
    }
}

#[test]
fn yamlfmt_numeric_options_follow_runtime_radix_and_truncation_rules() {
    for (indent, width, expected_indent, expected_width) in [
        ("010", "0120", 8, 80),
        ("+010", "+0120", 8, 80),
        ("0_10", "01_20", 8, 80),
        ("0b100", "0b1010000", 4, 80),
        ("0B100", "0X50", 4, 80),
        ("0o10", "0o120", 8, 80),
        ("+0o10", "+0O120", 8, 80),
        ("0o_10", "0o1_20", 8, 80),
        ("3.5", "40.9", 3, 40),
        ("3.0", "8e1", 3, 80),
        ("!!int 010", "!!float 0120", 8, 80),
        ("2", "0_8", 2, 8),
        ("2", "1_0e1", 2, 100),
        ("3_0.5", "4_0.9", 2, 40),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        fs::write(
            &source,
            format!("formatter: {{indent: {indent}, max_line_length: {width}}}\n"),
        )
        .unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
        let config = converted(&result);
        assert_eq!(
            ryl::format::indent_width(&config),
            expected_indent,
            "{indent}"
        );
        assert_eq!(ryl::format::line_length(&config), expected_width, "{width}");
    }
}

#[test]
fn yamlfmt_numeric_normalization_preserves_anchors_tags_comments_and_endings() {
    for ending in ["\n", "\r\n", "\r"] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        let body = "\u{feff}formatter: # café\n  indent: &spaces 010 # preserve anchor\n  array_indent: *spaces\n  max_line_length: !!int 0120\nunknown: {indent: '010'}\n".replace('\n', ending);
        fs::write(&source, &body).unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
        let config = converted(&result);
        assert_eq!(ryl::format::indent_width(&config), 8);
        assert_eq!(ryl::format::line_length(&config), 80);
        assert!(
            !result
                .warnings
                .iter()
                .any(|warning| warning.contains("array_indent"))
        );
        assert_eq!(fs::read_to_string(source).unwrap(), body);
    }
}

#[test]
fn unsupported_yamlfmt_numeric_presentations_warn_and_retain_all_sources() {
    for value in [
        "!!int '010'",
        "!!int \"010\"",
        "!!int |-\n    010",
        "01000000000000000000000000",
        "0b_",
        "0o_",
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        let body = format!("formatter:\n  indent: {value}\n");
        fs::write(&source, &body).unwrap();
        let mut opts = options(dir.path(), Some(MigrationSource::Yamlfmt));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        let result = migrate_configs(&opts).unwrap();
        assert!(result.entries.is_empty(), "{value}: {result:?}");
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("decimal")),
            "{result:?}"
        );
        assert_eq!(fs::read_to_string(source).unwrap(), body);
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn yaml_numeric_validation_keeps_prettier_core_resolution_and_rejects_invalid_tag_values()
 {
    let dir = tempdir().unwrap();
    let prettier = dir.path().join(".prettierrc.yaml");
    fs::write(&prettier, "tabWidth: 010\nprintWidth: 0120\n").unwrap();
    let result =
        migrate_configs(&options(&prettier, Some(MigrationSource::Prettier))).unwrap();
    assert_eq!(ryl::format::indent_width(&converted(&result)), 10);
    assert_eq!(ryl::format::line_length(&converted(&result)), 120);
    let yamlfmt = dir.path().join(".yamlfmt");
    for body in [
        "formatter: {indent: 2, max_line_length: -010}",
        "formatter: {indent: 2, max_line_length: -0o10}",
        "formatter: {indent: !!int '2', max_line_length: !!float '80.0'}",
    ] {
        fs::write(&yamlfmt, body).unwrap();
        assert_eq!(
            migrate_configs(&options(&yamlfmt, Some(MigrationSource::Yamlfmt)))
                .unwrap()
                .entries
                .len(),
            1
        );
    }
    for body in [
        "formatter: {indent: 1:20}",
        "formatter: {max_line_length: +.inf}",
        "formatter: {max_line_length: +.iNF}",
        "formatter: {indent: 1x}",
        "formatter: {indent: !!int 0_8}",
    ] {
        fs::write(&yamlfmt, body).unwrap();
        assert!(
            migrate_configs(&options(&yamlfmt, Some(MigrationSource::Yamlfmt)))
                .is_err(),
            "{body}"
        );
    }
}

fn options(root: &Path, source: Option<MigrationSource>) -> MigrateOptions {
    MigrateOptions {
        from: source,
        project_root: Some(root.to_path_buf()),
        user_config: None,
        write_mode: WriteMode::Preview,
        output_mode: OutputMode::IncludeToml,
        cleanup: SourceCleanup::Keep,
    }
}

fn converted(result: &MigrateResult) -> YamlLintConfig {
    assert_eq!(result.entries.len(), 1, "{result:?}");
    let file = tempfile::Builder::new()
        .suffix(".toml")
        .tempfile_in(result.entries[0].target.parent().unwrap())
        .unwrap();
    fs::write(file.path(), &result.entries[0].toml).unwrap();
    ryl::config::discover_config(
        &[],
        &ryl::config::Overrides {
            config_file: Some(file.path().to_path_buf()),
            config_data: None,
        },
    )
    .unwrap()
    .config
}

#[test]
fn effective_source_defaults_are_written_as_native_format_targets() {
    for (
        source,
        name,
        body,
        quote,
        keep,
        prose,
        fold,
        start,
        spaces,
        blanks,
        order,
        expand,
        ending,
    ) in [
        (
            MigrationSource::Prettier,
            ".prettierrc",
            "{}",
            QuoteStyleTarget::Double,
            true,
            Some(ProseWrap::Preserve),
            None,
            MarkerTarget::Preserve,
            1,
            1,
            PropertyOrderTarget::TagFirst,
            ExpandTarget::Auto,
            LineEndingTarget::Lf,
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            "{}",
            QuoteStyleTarget::Preserve,
            true,
            Some(ProseWrap::Never),
            None,
            MarkerTarget::Preserve,
            1,
            0,
            PropertyOrderTarget::AnchorFirst,
            ExpandTarget::Never,
            LineEndingTarget::Native,
        ),
        (
            MigrationSource::Dprint,
            "dprint.json",
            DPRINT,
            QuoteStyleTarget::Double,
            true,
            Some(ProseWrap::Preserve),
            None,
            MarkerTarget::Preserve,
            1,
            1,
            PropertyOrderTarget::Preserve,
            ExpandTarget::Preserve,
            LineEndingTarget::Lf,
        ),
        (
            MigrationSource::Yamlfix,
            ".yamlfix.toml",
            "",
            QuoteStyleTarget::Single,
            false,
            None,
            Some(true),
            MarkerTarget::Add,
            2,
            0,
            PropertyOrderTarget::AnchorFirst,
            ExpandTarget::Preserve,
            LineEndingTarget::Lf,
        ),
    ] {
        let dir = tempdir().unwrap();
        let path = dir.path().join(name);
        fs::write(&path, body).unwrap();
        let result = migrate_configs(&options(&path, Some(source))).unwrap();
        let config = converted(&result);
        let format = config.format().targets();
        assert_eq!(ryl::format::indent_width(&config), 2, "{source:?}");
        assert_eq!(ryl::format::line_length(&config), 80, "{source:?}");
        assert_eq!(format.quote_style, quote, "{source:?}");
        assert_eq!(format.keep_quotes, keep, "{source:?}");
        assert_eq!(format.prose_wrap, prose, "{source:?}");
        assert_eq!(format.fold_long_lines, fold, "{source:?}");
        assert_eq!(format.document_start, start, "{source:?}");
        assert_eq!(format.comment_spacing.get(), spaces, "{source:?}");
        assert_eq!(format.max_blank_lines, blanks, "{source:?}");
        assert_eq!(format.property_order, order, "{source:?}");
        assert_eq!(format.sequence_expand, expand, "{source:?}");
        assert_eq!(format.mapping_expand, expand, "{source:?}");
        assert_eq!(format.line_ending, ending, "{source:?}");
        assert_eq!(
            format.comment_starting_space,
            if source == MigrationSource::Yamlfix {
                MarkerTarget::Add
            } else {
                MarkerTarget::Preserve
            }
        );
        assert!(
            !config.enables_any_rule(),
            "formatter imports must not enable lint rules"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), body);
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn explicit_prettier_options_and_overrides_are_reported_without_execution() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".prettierrc.json5");
    fs::write(&source, "{tabWidth:4,printWidth:96,singleQuote:true,bracketSpacing:false,proseWrap:'always',endOfLine:'crlf',overrides:[{files:'*.yaml',options:{tabWidth:6}}],plugins:['not-executed']}").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Prettier))).unwrap();
    let config = converted(&result);
    let format = config.format().targets();
    assert_eq!(ryl::format::indent_width(&config), 6);
    assert_eq!(ryl::format::line_length(&config), 96);
    assert_eq!(format.quote_style, QuoteStyleTarget::Single);
    assert!(!format.brace_spacing);
    assert_eq!(format.prose_wrap, Some(ProseWrap::Always));
    assert_eq!(format.line_ending, LineEndingTarget::CrLf);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("plugins"))
    );
    assert!(result.entries[0].toml.contains("plugins"));
}

#[test]
fn formatter_dimensions_do_not_change_colocated_yamllint_policy() {
    let dir = tempdir().unwrap();
    let legacy = "extends: default\nrules:\n  line-length: {max: 12}\n";
    fs::write(dir.path().join(".yamllint"), legacy).unwrap();
    fs::write(
        dir.path().join(".prettierrc.json"),
        r#"{"tabWidth":4,"printWidth":96}"#,
    )
    .unwrap();
    let result = migrate_configs(&options(dir.path(), None)).unwrap();
    let after = converted(&result);
    let before = YamlLintConfig::from_yaml_str(legacy).unwrap();
    let input = "root:\n   child: value\ntext: words words words\n";
    let file = dir.path().join("input.yaml");
    assert_eq!(
        ryl::lint_str(input, &file, &before, dir.path()),
        ryl::lint_str(input, &file, &after, dir.path())
    );
    assert_eq!(ryl::format::line_length(&after), 96);
    assert_eq!(ryl::format::indent_width(&after), 4);
    assert_eq!(
        after.rule_option_str("indentation", "spaces"),
        Some("consistent")
    );
    assert_eq!(after.rule_option_int("line-length", "max", 0), 12);
}

#[test]
fn competing_formatters_require_selection_but_keep_yamllint() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}\n").unwrap();
    fs::write(dir.path().join(".yamlfmt"), "{}\n").unwrap();
    let error = migrate_configs(&options(dir.path(), None)).unwrap_err();
    assert!(error.contains("--migrate-from"), "{error}");
    assert!(!dir.path().join(".ryl.toml").exists());
    let result =
        migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier))).unwrap();
    let config = converted(&result);
    assert!(config.rule_level("truthy").is_some());
    assert_eq!(
        config.format().targets().quote_style,
        QuoteStyleTarget::Double
    );
    let lint_only = converted(
        &migrate_configs(&options(dir.path(), Some(MigrationSource::Yamllint)))
            .unwrap(),
    );
    assert_eq!(
        lint_only.format().targets().quote_style,
        QuoteStyleTarget::Preserve
    );
}

#[test]
fn cleanup_preserves_shared_sources_and_ignores_their_backup_collisions() {
    for (source, filename, body) in [
        (
            MigrationSource::Prettier,
            "package.json",
            r#"{"name":"kept","prettier":{"singleQuote":true}}"#,
        ),
        (
            MigrationSource::Prettier,
            ".prettierrc.yaml",
            "singleQuote: true\n",
        ),
        (MigrationSource::Dprint, "dprint.jsonc", DPRINT),
        (
            MigrationSource::Yamlfix,
            "pyproject.toml",
            "[project]\nname='kept'\n[tool.yamlfix]\nline_length=90\n",
        ),
    ] {
        for cleanup in [
            SourceCleanup::Delete,
            SourceCleanup::RenameSuffix(".bak".to_owned()),
        ] {
            let dir = tempdir().unwrap();
            let path = dir.path().join(filename);
            let backup = dir.path().join(format!("{filename}.bak"));
            fs::write(&path, body).unwrap();
            fs::write(&backup, "retained backup\n").unwrap();
            let mut opts = options(&path, Some(source));
            opts.write_mode = WriteMode::Write;
            opts.cleanup = cleanup;
            let result = migrate_configs(&opts).unwrap();
            converted(&result);
            assert_eq!(fs::read_to_string(&path).unwrap(), body);
            assert_eq!(fs::read_to_string(backup).unwrap(), "retained backup\n");
            assert!(dir.path().join(".ryl.toml").exists());
            assert!(!result.entries[0].cleanup_source);
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("cleanup skipped"))
            );
            assert!(result.entries[0].toml.contains("cleanup skipped"));
        }
    }
}

#[test]
fn source_selection_requires_project_migration() {
    let dir = tempdir().unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).args(["--migrate-from", "prettier"]));
    assert_eq!(code, 2);
    assert!(stderr.contains("--migrate-configs"));
}

fn json5_control(source: &str, accepted: bool, expected_indent: u8) {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".prettierrc.json5");
    fs::write(&path, source).unwrap();
    let result = migrate_configs(&options(&path, Some(MigrationSource::Prettier)));
    if accepted {
        let config = converted(&result.unwrap());
        assert_eq!(ryl::format::indent_width(&config), expected_indent);
    } else {
        assert!(result.is_err(), "{source:?}: {result:?}");
    }
    assert_eq!(fs::read_to_string(path).unwrap(), source);
    assert!(!dir.path().join(".ryl.toml").exists());
}

macro_rules! json5_control {
    ($name:ident, $source:expr, $accepted:expr, $indent:expr) => {
        #[test]
        fn $name() {
            json5_control($source, $accepted, $indent);
        }
    };
}

json5_control!(
    json5_bare_keys_and_single_quotes,
    "{singleQuote:true,proseWrap:'never',}",
    true,
    2
);
json5_control!(json5_hexadecimal_numbers, "{printWidth:0x50}", true, 2);
json5_control!(json5_unary_plus_numbers, "{printWidth:+80}", true, 2);
json5_control!(json5_bare_decimal_points, "{dummy:.5,dummy2:1.}", true, 2);
json5_control!(
    json5_extended_string_escapes,
    r"{endOfLine:'\x6c\x66',dummy:'\v\0'}",
    true,
    2
);
json5_control!(
    json5_escaped_physical_continuations,
    "{dummy:'a\\\nb'}",
    true,
    2
);
json5_control!(json5_unicode_identifiers, "{世:'value'}", true, 2);
json5_control!(
    json5_escaped_identifiers_map_the_decoded_option,
    r"{\u0074abWidth:4}",
    true,
    4
);
json5_control!(json5_joiner_identifiers, "{a\u{200c}b:'value'}", true, 2);
json5_control!(
    json5_nonfinite_values_are_not_null_coerced,
    "{dummy:NaN,other:Infinity}",
    false,
    2
);
json5_control!(
    json5_comments_and_trailing_commas,
    "{// test\nprintWidth:80,/* hi */tabWidth:2,}",
    true,
    2
);
json5_control!(
    json5_missing_commas_are_rejected,
    "{printWidth:80 tabWidth:2}",
    false,
    2
);
json5_control!(
    json5_hyphen_identifiers_are_rejected,
    "{invalid-key:1}",
    false,
    2
);
json5_control!(json5_numeric_identifiers_are_rejected, "{123:1}", false, 2);

#[test]
fn json5_scalar_types_and_integer_valued_floats_are_preserved() {
    json5_control(
        "{tabWidth:4.,printWidth:8e1,dummy:[null,false,-7,18446744073709551615,'borrowed','escaped\\x41']}",
        true,
        4,
    );
}

#[test]
fn unrepresentable_integral_source_numbers_warn_and_use_a_supported_default() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(".prettierrc.json5");
    fs::write(&path, "{printWidth:1e40,tabWidth:300}").unwrap();
    let result =
        migrate_configs(&options(&path, Some(MigrationSource::Prettier))).unwrap();
    let config = converted(&result);
    assert_eq!(ryl::format::line_length(&config), 80);
    assert_eq!(ryl::format::indent_width(&config), 2);
    for key in ["printWidth", "tabWidth"] {
        assert!(result.warnings.iter().any(|warning| warning.contains(key)));
        assert!(result.entries[0].toml.contains(key));
    }
}

#[test]
fn json5_depth_limit_bounds_the_complete_cli_parse_path() {
    for (depth, accepted) in [(127, true), (128, false), (100_000, false)] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".prettierrc.json5");
        let nested = format!("{}0{}", "{x:".repeat(depth), "}".repeat(depth));
        fs::write(&source, format!("{{unknown:{nested}}}")).unwrap();
        let (code, stdout, stderr) = run(ryl(dir.path())
            .args([
                "--migrate-configs",
                "--migrate-from",
                "prettier",
                "--migrate-root",
            ])
            .arg(&source));
        assert_eq!(
            code,
            if accepted { 0 } else { 2 },
            "depth={depth}: {stdout} {stderr}"
        );
        if !accepted {
            assert!(stderr.contains("at most 128 nested containers"), "{stderr}");
        }
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn dprint_jsonc_extensions_match_the_latest_source_controls() {
    for source in [
        r"{plugins:['pretty_yaml.wasm'],yaml:{printWidth:80}}",
        r#"{"plugins":["pretty_yaml.wasm"] "yaml":{"printWidth":80}}"#,
        r"{plugins:['pretty_yaml.wasm'],yaml:{printWidth:+80}}",
        "{// comment\n\"plugins\":[\"pretty_yaml.wasm\"],\"yaml\":{\"printWidth\":80,},}",
    ] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dprint.json");
        fs::write(&path, source).unwrap();
        let result =
            migrate_configs(&options(&path, Some(MigrationSource::Dprint))).unwrap();
        assert_eq!(ryl::format::line_length(&converted(&result)), 80);
    }
    for value in ["0x50", "80.0", "8e1", "80.", "NaN", "Infinity", "'80'"] {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dprint.jsonc");
        fs::write(
            &path,
            format!("{{plugins:['pretty_yaml.wasm'],yaml:{{printWidth:{value}}}}}"),
        )
        .unwrap();
        let error = migrate_configs(&options(&path, Some(MigrationSource::Dprint)))
            .unwrap_err();
        assert!(error.contains("invalid config"), "{value}: {error}");
    }
}

#[test]
fn dynamic_prettier_and_shared_packages_do_not_fall_through_or_execute() {
    for shared in [false, true] {
        let dir = tempdir().unwrap();
        let marker = dir.path().join("must-not-exist");
        let source = if shared {
            dir.path().join("package.json")
        } else {
            dir.path().join(".prettierrc.js")
        };
        let body = if shared {
            r#"{"prettier":"not-imported"}"#.to_owned()
        } else {
            format!(
                "require('fs').writeFileSync({},'executed');module.exports={{tabWidth:6}}",
                serde_json::to_string(&marker.to_string_lossy()).unwrap()
            )
        };
        fs::write(&source, body).unwrap();
        fs::write(dir.path().join(".prettierrc.toml"), "tabWidth=6\n").unwrap();
        let result =
            migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier)))
                .unwrap();
        assert!(result.entries.is_empty());
        assert!(!result.warnings.is_empty());
        assert!(!marker.exists());
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn unrelated_shared_sources_and_non_yaml_dprint_plugins_are_ignored() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"project"}"#).unwrap();
    fs::write(dir.path().join("package.yaml"), "name: project\n").unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname='project'\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("dprint.json"),
        r#"{"plugins":["typescript.wasm"],"typescript":{}}"#,
    )
    .unwrap();
    let result = migrate_configs(&options(dir.path(), None)).unwrap();
    assert!(result.entries.is_empty(), "{result:?}");
    assert!(!dir.path().join(".ryl.toml").exists());
}

#[cfg(unix)]
#[test]
fn cleanup_uses_the_source_action_path_without_resolving_parent_segments_lexically() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let logical = dir.path().join("logical");
    let actual = dir.path().join("actual");
    fs::create_dir_all(actual.join("nested")).unwrap();
    fs::create_dir(&logical).unwrap();
    symlink(actual.join("nested"), logical.join("link")).unwrap();
    let source = logical.join("link/../yamlfix.toml");
    let sentinel = logical.join("yamlfix.toml");
    fs::write(actual.join("yamlfix.toml"), "line_length = 90\n").unwrap();
    fs::write(&sentinel, "line_length = 120\n").unwrap();
    let mut opts = options(&source, Some(MigrationSource::Yamlfix));
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    let result = migrate_configs(&opts).unwrap();
    assert_eq!(ryl::format::line_length(&converted(&result)), 90);
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "line_length = 120\n");
    assert!(!actual.join("yamlfix.toml").exists());
    assert!(actual.join(".ryl.toml").exists());
}

#[test]
fn cleanup_retains_unknown_sections_and_custom_source_names() {
    for (tool, name, body) in [
        (
            MigrationSource::Yamlfix,
            "yamlfix.toml",
            "line_length=90\n[project]\nname='shared'\n",
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            "formatter: {indent: 4}\nother: {language: true}\n",
        ),
        (
            MigrationSource::Yamlfmt,
            "custom.yaml",
            "formatter: {indent: 4}\n",
        ),
        (MigrationSource::Yamlfix, "custom.toml", "line_length=90\n"),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(name);
        fs::write(&source, body).unwrap();
        let mut opts = options(&source, Some(tool));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        let result = migrate_configs(&opts).unwrap();
        assert_eq!(result.entries.len(), 1, "{name}: {result:?}");
        assert_eq!(fs::read_to_string(source).unwrap(), body);
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("cleanup skipped")),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn nonfinite_toml_values_are_rejected_without_writing_or_deleting_sources() {
    for value in [
        "nan",
        "+nan",
        "-nan",
        "inf",
        "+inf",
        "-inf",
        "[1, nan]",
        "{nested=inf}",
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join("yamlfix.toml");
        let body = format!("unknown={value}\n");
        fs::write(&source, &body).unwrap();
        let mut opts = options(&source, Some(MigrationSource::Yamlfix));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        let error = migrate_configs(&opts).unwrap_err();
        assert!(error.contains("non-finite"), "{value}: {error}");
        assert_eq!(fs::read_to_string(source).unwrap(), body);
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn explicit_yamlfmt_policies_map_with_specific_gap_warnings() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".yamlfmt");
    fs::write(&source, "line_ending: crlf\nformatter:\n  type: basic\n  indent: 4\n  max_line_length: 100\n  force_quote_style: single\n  include_document_start: true\n  retain_line_breaks: true\n  retain_line_breaks_single: true\n  force_array_style: block\n  indentless_arrays: true\n  array_indent: 6\n  indent_root_array: true\n  pad_line_comments: 3\n  scan_folded_as_literal: true\n  disallow_anchors: true\nexclude: [vendor]\n").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
    let config = converted(&result);
    let format = config.format().targets();
    assert_eq!(ryl::format::indent_width(&config), 4);
    assert_eq!(ryl::format::line_length(&config), 100);
    assert_eq!(format.line_ending, LineEndingTarget::CrLf);
    assert_eq!(format.quote_style, QuoteStyleTarget::Single);
    assert_eq!(format.document_start, MarkerTarget::Add);
    assert_eq!(format.prose_wrap, Some(ProseWrap::Preserve));
    assert_eq!(
        format.sequence_style,
        ryl::config_schema::CollectionStyleTarget::Block
    );
    assert!(!format.indent_sequences);
    assert_eq!(format.comment_spacing.get(), 3);
    assert_eq!(format.max_blank_lines, 1);
    for key in [
        "array_indent",
        "indent_root_array",
        "scan_folded_as_literal",
        "disallow_anchors",
        "exclude",
    ] {
        assert!(
            result.warnings.iter().any(|warning| warning.contains(key)),
            "{key}: {result:?}"
        );
    }
}

#[test]
fn explicit_dprint_globals_and_yaml_overrides_map_independently() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("dprint.json");
    fs::write(&source, r#"{plugins:['pretty_yaml.wasm'],indentWidth:6,lineWidth:120,newLineKind:'crlf',yaml:{indentWidth:4,printWidth:96,lineBreak:'lf',quotes:'forceSingle',formatComments:true,braceSpacing:false,bracketSpacing:true,dashSpacing:'indent',preferSingleLine:true,'flowSequence.preferSingleLine':false,'flowMap.preferSingleLine':null,indentBlockSequenceInMap:false,proseWrap:'always',trailingComma:false,trimTrailingWhitespaces:false,trimTrailingZero:true,ignoreCommentDirective:'custom-ignore'}}"#).unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Dprint))).unwrap();
    let config = converted(&result);
    let format = config.format().targets();
    assert_eq!(ryl::format::indent_width(&config), 4);
    assert_eq!(ryl::format::line_length(&config), 96);
    assert_eq!(format.line_ending, LineEndingTarget::Lf);
    assert_eq!(format.quote_style, QuoteStyleTarget::Single);
    assert_eq!(format.comment_starting_space, MarkerTarget::Add);
    assert!(!format.brace_spacing);
    assert!(!format.indent_sequences);
    assert_eq!(format.sequence_expand, ExpandTarget::Preserve);
    assert_eq!(format.mapping_expand, ExpandTarget::Auto);
    assert_eq!(format.prose_wrap, Some(ProseWrap::Always));
    for key in [
        "quotes",
        "bracketSpacing",
        "dashSpacing",
        "trailingComma",
        "trimTrailingWhitespaces",
        "trimTrailingZero",
        "custom-ignore",
    ] {
        assert!(
            result.warnings.iter().any(|warning| warning.contains(key)),
            "{key}: {result:?}"
        );
    }
}

#[test]
fn yamlfix_preservation_and_sequence_styles_use_bare_toml_fields() {
    for (style, expected) in [
        (
            "flow_style",
            ryl::config_schema::CollectionStyleTarget::Flow,
        ),
        (
            "block_style",
            ryl::config_schema::CollectionStyleTarget::Block,
        ),
        (
            "keep_style",
            ryl::config_schema::CollectionStyleTarget::Preserve,
        ),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join("yamlfix.toml");
        fs::write(&source, format!("line_length=100\nindent_mapping=4\nindent_sequence=4\nindent_offset=0\npreserve_quotes=true\nquote_representation='\"'\nexplicit_start=false\ncomments_min_spaces_from_content=3\ncomments_require_starting_space=false\nwhitelines=2\ncomments_whitelines=0\nsection_whitelines=0\nsequence_style='{style}'\nquote_basic_values=true\nquote_keys_and_basic_values=true\nnone_representation='null'\nallow_duplicate_keys=true\n")).unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfix))).unwrap();
        let config = converted(&result);
        let format = config.format().targets();
        assert_eq!(ryl::format::indent_width(&config), 4);
        assert_eq!(ryl::format::line_length(&config), 100);
        assert_eq!(format.quote_style, QuoteStyleTarget::Preserve);
        assert!(format.keep_quotes);
        assert_eq!(format.document_start, MarkerTarget::Preserve);
        assert_eq!(format.comment_starting_space, MarkerTarget::Preserve);
        assert_eq!(format.comment_spacing.get(), 3);
        assert_eq!(format.max_blank_lines, 2);
        assert_eq!(format.sequence_style, expected);
        assert!(!format.indent_sequences);
        for key in [
            "quote_basic_values",
            "quote_keys_and_basic_values",
            "none_representation",
            "whitelines",
        ] {
            assert!(
                result.warnings.iter().any(|warning| warning.contains(key)),
                "{key}: {result:?}"
            );
        }
    }
}

#[test]
fn ill_typed_source_options_fail_before_any_migration_write() {
    for (tool, name, bodies) in [
        (
            MigrationSource::Prettier,
            ".prettierrc.json",
            vec![
                r#"{"tabWidth":false}"#,
                r#"{"singleQuote":1}"#,
                r#"{"proseWrap":"invalid"}"#,
                r#"{"endOfLine":123}"#,
            ],
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            vec![
                "formatter: []",
                "formatter: {indent: true}",
                "line_ending: invalid",
                "formatter: {force_array_style: invalid}",
            ],
        ),
        (
            MigrationSource::Dprint,
            "dprint.json",
            vec![
                "{yaml:[]}",
                "{yaml:{formatComments:1}}",
                "{yaml:{'flowMap.preferSingleLine':1}}",
                "{yaml:{quotes:'invalid'}}",
            ],
        ),
        (
            MigrationSource::Yamlfix,
            "yamlfix.toml",
            vec![
                "preserve_quotes=1",
                "sequence_style='invalid'",
                "quote_representation='invalid'",
                "line_length=1.5",
            ],
        ),
    ] {
        for body in bodies {
            let dir = tempdir().unwrap();
            let source = dir.path().join(name);
            fs::write(&source, body).unwrap();
            let mut opts = options(&source, Some(tool));
            opts.write_mode = WriteMode::Write;
            opts.cleanup = SourceCleanup::Delete;
            assert!(migrate_configs(&opts).is_err(), "{tool:?}: {body}");
            assert_eq!(fs::read_to_string(&source).unwrap(), body);
            assert!(!dir.path().join(".ryl.toml").exists());
        }
    }
}

#[test]
fn source_filename_priority_and_yamlfix_ancestor_merging_match_the_tools() {
    let dir = tempdir().unwrap();
    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.yamlfix]\nline_length=100\nindent_mapping=4\n",
    )
    .unwrap();
    fs::write(dir.path().join("yamlfix.toml"), "line_length=110\n").unwrap();
    fs::write(child.join(".yamlfix.toml"), "line_length=120\n").unwrap();
    let result =
        migrate_configs(&options(&child, Some(MigrationSource::Yamlfix))).unwrap();
    let config = converted(&result);
    assert_eq!(ryl::format::indent_width(&config), 4);
    assert_eq!(ryl::format::line_length(&config), 120);
    for (tool, files, expected) in [
        (
            MigrationSource::Prettier,
            vec![
                (".prettierrc", "tabWidth: 3"),
                (".prettierrc.json", "{\"tabWidth\":4}"),
            ],
            3,
        ),
        (
            MigrationSource::Dprint,
            vec![
                ("dprint.json", "{yaml:{indentWidth:3}}"),
                ("dprint.jsonc", "{yaml:{indentWidth:4}}"),
                (".dprint.json", "{yaml:{indentWidth:5}}"),
                (".dprint.jsonc", "{yaml:{indentWidth:6}}"),
            ],
            3,
        ),
    ] {
        let dir = tempdir().unwrap();
        for (name, body) in files {
            fs::write(dir.path().join(name), body).unwrap();
        }
        let result = migrate_configs(&options(dir.path(), Some(tool))).unwrap();
        assert_eq!(ryl::format::indent_width(&converted(&result)), expected);
    }
}

#[test]
fn competing_yamlfmt_filenames_require_an_explicit_file() {
    let dir = tempdir().unwrap();
    for name in [
        ".yamlfmt",
        "yamlfmt.yaml",
        "yamlfmt.yml",
        ".yamlfmt.yaml",
        ".yamlfmt.yml",
    ] {
        fs::write(dir.path().join(name), "formatter: {indent: 4}\n").unwrap();
        let result = migrate_configs(&options(
            &dir.path().join(name),
            Some(MigrationSource::Yamlfmt),
        ))
        .unwrap();
        assert_eq!(ryl::format::indent_width(&converted(&result)), 4);
    }
    let error = migrate_configs(&options(dir.path(), Some(MigrationSource::Yamlfmt)))
        .unwrap_err();
    assert!(error.contains("--migrate-root"));
}

#[test]
fn source_warnings_cannot_inject_toml_or_terminal_control_lines() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".prettierrc.json");
    fs::write(
        &source,
        r#"{"unknown\n[lint]\nrules.ALL=\"enable\"\u001b[31m":"\n\r\u0000"}"#,
    )
    .unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Prettier))).unwrap();
    assert!(!converted(&result).enables_any_rule());
    for warning in &result.warnings {
        assert!(
            !warning.contains(['\n', '\r', '\0', '\u{1b}']),
            "{warning:?}"
        );
    }
    assert!(result.entries[0].toml.contains("\\u{a}[lint]"));

    fs::write(&source, r#"{"unknown":"ryl's"}"#).unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Prettier))).unwrap();
    for output in [&result.warnings.join("\n"), &result.entries[0].toml] {
        assert!(output.contains(r#"unknown = "ryl's""#), "{output}");
    }

    let source = dir.path().join("pyproject.toml");
    fs::write(&source, "[tool.yamlfix]\nline_length = 100\n").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Yamlfix))).unwrap();
    for output in [&result.warnings.join("\n"), &result.entries[0].toml] {
        assert!(output.contains(r#"none_representation = """#), "{output}");
        assert!(output.contains("ryl's"), "{output}");
    }
}

#[test]
fn static_prettier_formats_and_empty_sources_share_effective_defaults() {
    for (name, body) in [
        (".prettierrc", "\u{feff}tabWidth: 3\n"),
        (".prettierrc.yaml", "tabWidth: 3\n"),
        (".prettierrc.yml", "tabWidth: 3\n"),
        (".prettierrc.json", "{\"tabWidth\":3}"),
        (".prettierrc.toml", "tabWidth=3\n"),
        ("package.yaml", "prettier: {tabWidth: 3}\n"),
    ] {
        let dir = tempdir().unwrap();
        let path = dir.path().join(name);
        fs::write(&path, body).unwrap();
        let result =
            migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier)))
                .unwrap();
        assert_eq!(ryl::format::indent_width(&converted(&result)), 3, "{name}");
    }
    for body in ["", "null", "false", "0", "''", "{}", "[]"] {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".prettierrc");
        fs::write(&path, body).unwrap();
        let result =
            migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier)))
                .unwrap();
        assert_eq!(ryl::format::indent_width(&converted(&result)), 2, "{body}");
    }
}

#[test]
fn unsupported_yaml_values_and_multiple_documents_are_not_coerced() {
    for body in [
        "true",
        "2",
        "1: value",
        "true: value",
        "null: value",
        "? [a, b]\n: value",
        "tabWidth: .nan",
        "tabWidth: .inf",
        "tabWidth: !custom 3",
        "---\n{}\n---\n{}\n",
    ] {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".prettierrc.yaml");
        fs::write(&path, body).unwrap();
        assert!(
            migrate_configs(&options(&path, Some(MigrationSource::Prettier))).is_err(),
            "{body}"
        );
    }
}

#[test]
fn representability_gaps_keep_valid_native_fallbacks_and_report_both_channels() {
    for (tool, filename, body, expected_indent, keys) in [
        (
            MigrationSource::Prettier,
            ".prettierrc.json5",
            "{tabWidth:0,printWidth:999999,endOfLine:'cr',requirePragma:true,insertPragma:true,checkIgnorePragma:true}",
            2,
            vec![
                "tabWidth",
                "printWidth",
                "endOfLine",
                "requirePragma",
                "insertPragma",
                "checkIgnorePragma",
            ],
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            "formatter: {indent: 0, max_line_length: -1, pad_line_comments: 0, retain_line_breaks: true, force_array_style: flow, force_quote_style: double}",
            4,
            vec![
                "max_line_length",
                "pad_line_comments",
                "retain_line_breaks",
                "force_array_style",
            ],
        ),
        (
            MigrationSource::Yamlfix,
            "yamlfix.toml",
            "indent_mapping=0\nline_length=999999\ncomments_min_spaces_from_content=-1\nwhitelines=999\nquote_representation='\"'\nconfig_path='ignored'\n",
            2,
            vec![
                "indent_mapping",
                "line_length",
                "comments_min_spaces_from_content",
                "whitelines",
                "config_path",
            ],
        ),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(filename);
        fs::write(&source, body).unwrap();
        let result = migrate_configs(&options(&source, Some(tool))).unwrap();
        let config = converted(&result);
        assert_eq!(ryl::format::indent_width(&config), expected_indent);
        assert_eq!(ryl::format::line_length(&config), 80);
        for key in keys {
            assert!(
                result.warnings.iter().any(|warning| warning.contains(key)),
                "{key}: {result:?}"
            );
            assert!(result.entries[0].toml.contains(key), "{key}: {result:?}");
        }
    }
}

#[test]
fn static_yaml_aliases_resolve_without_losing_source_types() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".prettierrc.yaml");
    fs::write(&source, "tabWidth: &width 4\nprintWidth: *width\nunknown: [null, true, 1.5, {nested: text}]\n").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Prettier))).unwrap();
    let config = converted(&result);
    assert_eq!(ryl::format::indent_width(&config), 4);
    assert_eq!(ryl::format::line_length(&config), 4);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown"))
    );
}

#[test]
fn empty_packages_do_not_shadow_a_lower_prettier_config() {
    for value in ["null", "false", "0", "\"\""] {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            format!("{{\"prettier\":{value}}}"),
        )
        .unwrap();
        fs::write(dir.path().join(".prettierrc.json"), "{\"tabWidth\":4}").unwrap();
        let result =
            migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier)))
                .unwrap();
        assert_eq!(ryl::format::indent_width(&converted(&result)), 4);
    }
}

#[test]
fn prettier_null_options_use_their_source_defaults_without_hiding_unknown_nulls() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".prettierrc.json5");
    fs::write(&source, "{tabWidth:null,printWidth:null,singleQuote:null,bracketSpacing:null,proseWrap:null,endOfLine:null,useTabs:null,unknown:null}").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Prettier))).unwrap();
    let config = converted(&result);
    assert_eq!(ryl::format::indent_width(&config), 2);
    assert_eq!(ryl::format::line_length(&config), 80);
    let format = config.format().targets();
    assert_eq!(format.quote_style, QuoteStyleTarget::Double);
    assert!(format.brace_spacing);
    assert_eq!(format.prose_wrap, Some(ProseWrap::Preserve));
    assert_eq!(format.line_ending, LineEndingTarget::Lf);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown"))
    );
}

#[test]
fn prettier_array_configuration_retains_unknown_indexed_options_with_a_warning() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".prettierrc.json");
    fs::write(&source, "[4,true,{\"tabWidth\":3}]").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Prettier))).unwrap();
    assert_eq!(ryl::format::indent_width(&converted(&result)), 2);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("option 0"))
    );
}

#[test]
fn unrelated_shared_files_do_not_validate_a_native_config_during_its_own_migration() {
    let dir = tempdir().unwrap();
    let config = "[tool.ryl]\nyaml-files=['*.yaml']\n";
    fs::write(dir.path().join("pyproject.toml"), config).unwrap();
    fs::write(dir.path().join("package.json"), "{\"name\":\"project\"}").unwrap();
    let result = migrate_configs(&options(dir.path(), None)).unwrap();
    assert!(result.entries.is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("yaml-files"))
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("pyproject.toml")).unwrap(),
        config
    );
}

#[cfg(unix)]
#[test]
fn formatter_source_symlinks_are_retained_without_importing_settings() {
    use std::os::unix::fs::symlink;

    for (tool, filename, body) in [
        (
            MigrationSource::Prettier,
            "package.json",
            "{\"prettier\":{}}",
        ),
        (MigrationSource::Yamlfmt, ".yamlfmt", "{}"),
        (MigrationSource::Dprint, "dprint.json", DPRINT),
        (
            MigrationSource::Yamlfix,
            "pyproject.toml",
            "[tool.yamlfix]\nline_length=90\n",
        ),
        (MigrationSource::Yamlfix, "yamlfix.toml", "line_length=90\n"),
    ] {
        let dir = tempdir().unwrap();
        let real = dir.path().join("source-data");
        let source = dir.path().join(filename);
        fs::write(&real, body).unwrap();
        symlink(&real, &source).unwrap();
        for root in [dir.path(), source.as_path()] {
            let mut opts = options(root, Some(tool));
            opts.write_mode = WriteMode::Write;
            opts.cleanup = SourceCleanup::Delete;
            let result = migrate_configs(&opts).unwrap();
            assert!(result.entries.is_empty(), "{tool:?}: {result:?}");
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("symlink"))
            );
            assert!(source.is_symlink());
            assert_eq!(fs::read_to_string(&real).unwrap(), body);
            assert!(!dir.path().join(".ryl.toml").exists());
        }
    }
}

#[test]
fn json5_and_jsonc_container_limits_bound_arrays_and_objects() {
    for (tool, filename, prefix) in [
        (MigrationSource::Prettier, ".prettierrc.json5", "{unknown:"),
        (MigrationSource::Dprint, "dprint.jsonc", "{yaml:{},unknown:"),
    ] {
        for (open, close) in [("[", "]"), ("{x:", "}")] {
            for (depth, accepted) in [(127, true), (128, false)] {
                let dir = tempdir().unwrap();
                let source = dir.path().join(filename);
                fs::write(
                    &source,
                    format!("{prefix}{}0{}}}", open.repeat(depth), close.repeat(depth)),
                )
                .unwrap();
                let result = migrate_configs(&options(&source, Some(tool)));
                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "{tool:?} {open} depth={depth}: {result:?}"
                );
                if let Err(error) = result {
                    assert!(error.contains("at most 128 nested containers"), "{error}");
                }
            }
        }
    }
}

#[test]
fn jsonc_unknown_options_preserve_finite_scalar_and_container_types_in_warnings() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("dprint.jsonc");
    fs::write(&source, r#"{yaml:{},unknown:[null,false,-7,18446744073709551615,0.5,'borrowed','escaped\n',{nested:true}]}"#).unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Dprint))).unwrap();
    converted(&result);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("unknown") && warning.contains("0.5"))
    );
}

#[test]
fn each_typed_formatter_option_rejects_an_object_instead_of_its_declared_type() {
    for (tool, filename, section, keys) in [
        (
            MigrationSource::Prettier,
            ".prettierrc.json",
            None,
            vec![
                "tabWidth",
                "printWidth",
                "singleQuote",
                "bracketSpacing",
                "proseWrap",
                "endOfLine",
                "useTabs",
            ],
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            Some("formatter"),
            vec![
                "type",
                "indent",
                "max_line_length",
                "force_quote_style",
                "include_document_start",
                "retain_line_breaks",
                "retain_line_breaks_single",
                "array_indent",
                "indent_root_array",
                "indentless_arrays",
                "force_array_style",
                "pad_line_comments",
                "scan_folded_as_literal",
            ],
        ),
        (
            MigrationSource::Dprint,
            "dprint.json",
            Some("yaml"),
            vec![
                "indentWidth",
                "printWidth",
                "lineBreak",
                "quotes",
                "formatComments",
                "indentBlockSequenceInMap",
                "braceSpacing",
                "bracketSpacing",
                "dashSpacing",
                "preferSingleLine",
                "flowSequence.preferSingleLine",
                "flowMap.preferSingleLine",
                "proseWrap",
                "trailingComma",
                "trimTrailingWhitespaces",
                "trimTrailingZero",
                "ignoreCommentDirective",
            ],
        ),
        (
            MigrationSource::Dprint,
            "dprint.json",
            None,
            vec!["indentWidth", "lineWidth", "newLineKind", "useTabs"],
        ),
    ] {
        for key in keys {
            let dir = tempdir().unwrap();
            let source = dir.path().join(filename);
            let mut inner = serde_json::Map::new();
            inner.insert(key.to_owned(), serde_json::json!({"invalid":true}));
            let mut value = serde_json::Value::Object(inner);
            if let Some(section) = section {
                value = serde_json::json!({section:value});
            }
            if tool == MigrationSource::Dprint {
                value
                    .as_object_mut()
                    .unwrap()
                    .entry("yaml")
                    .or_insert(serde_json::json!({}));
            }
            fs::write(&source, value.to_string()).unwrap();
            let error = migrate_configs(&options(&source, Some(tool))).unwrap_err();
            assert!(error.contains(key), "{tool:?} {key}: {error}");
            assert!(!dir.path().join(".ryl.toml").exists());
        }
    }
    for key in [
        "indent_mapping",
        "line_length",
        "indent_sequence",
        "indent_offset",
        "preserve_quotes",
        "quote_representation",
        "explicit_start",
        "comments_min_spaces_from_content",
        "comments_require_starting_space",
        "whitelines",
        "comments_whitelines",
        "section_whitelines",
        "none_representation",
        "sequence_style",
        "quote_basic_values",
        "quote_keys_and_basic_values",
        "allow_duplicate_keys",
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join("yamlfix.toml");
        fs::write(&source, format!("{key}={{invalid=true}}\n")).unwrap();
        let error = migrate_configs(&options(&source, Some(MigrationSource::Yamlfix)))
            .unwrap_err();
        assert!(error.contains(key), "{key}: {error}");
    }
}

#[test]
fn source_ending_prose_and_plugin_only_defaults_cover_alternative_policies() {
    for (tool, name, body, ending, prose) in [
        (
            MigrationSource::Prettier,
            ".prettierrc.json",
            r#"{"endOfLine":"auto","proseWrap":"never"}"#,
            LineEndingTarget::Auto,
            ProseWrap::Never,
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            "line_ending: lf\nformatter: {max_line_length: 90, force_quote_style: double}\n",
            LineEndingTarget::Lf,
            ProseWrap::Always,
        ),
        (
            MigrationSource::Dprint,
            "dprint.json",
            "{plugins:['pretty_yaml.wasm'],newLineKind:'crlf'}",
            LineEndingTarget::CrLf,
            ProseWrap::Preserve,
        ),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(name);
        fs::write(&source, body).unwrap();
        let result = migrate_configs(&options(&source, Some(tool))).unwrap();
        let config = converted(&result);
        assert_eq!(config.format().targets().line_ending, ending);
        assert_eq!(config.format().targets().prose_wrap, Some(prose));
    }
}

#[test]
fn unsupported_yamlfmt_formatter_types_are_reported_without_default_imports() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".yamlfmt");
    fs::write(&source, "formatter: {type: custom, indent: 4}\n").unwrap();
    let mut opts = options(&source, Some(MigrationSource::Yamlfmt));
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    let result = migrate_configs(&opts).unwrap();
    assert!(result.entries.is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("only yamlfmt"))
    );
    assert!(source.exists());
    assert!(!dir.path().join(".ryl.toml").exists());
}

#[test]
fn colocated_lint_defaults_integer_indentation_and_warning_levels_survive_formatter_dimensions()
 {
    for body in [
        "rules: {line-length: enable, indentation: enable}\n",
        "rules: {line-length: {level: warning, max: 70}, indentation: {level: warning, spaces: 4}}\n",
        "rules: {truthy: enable}\n",
    ] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".yamllint"), body).unwrap();
        fs::write(dir.path().join(".yamllint.yaml"), "rules: {}\n").unwrap();
        fs::write(
            dir.path().join(".prettierrc"),
            "tabWidth: 3\nprintWidth: 90\n",
        )
        .unwrap();
        let result = migrate_configs(&options(dir.path(), None)).unwrap();
        let config = converted(&result);
        let before = YamlLintConfig::from_yaml_str(body).unwrap();
        let input = "root:\n    child: yes\ntext: words words words words words words words words words words words words words words words\n";
        assert_eq!(
            ryl::lint_str(input, Path::new("input.yaml"), &before, dir.path()),
            ryl::lint_str(input, Path::new("input.yaml"), &config, dir.path())
        );
        assert_eq!(ryl::format::indent_width(&config), 3);
        assert_eq!(ryl::format::line_length(&config), 90);
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("lower-precedence"))
        );
    }
}

#[test]
fn static_parser_failures_do_not_create_targets() {
    for (tool, name, body) in [
        (MigrationSource::Prettier, ".prettierrc.json", "{"),
        (MigrationSource::Prettier, ".prettierrc.yaml", "tabWidth: ["),
        (MigrationSource::Prettier, ".prettierrc.toml", "tabWidth=["),
        (
            MigrationSource::Prettier,
            ".prettierrc.yaml",
            "tabWidth: !!int invalid",
        ),
        (MigrationSource::Yamlfmt, ".yamlfmt", "[]"),
        (MigrationSource::Yamlfmt, ".yamlfmt", "formatter: ["),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            "formatter: {indent: !custom 4}",
        ),
        (MigrationSource::Yamlfix, "yamlfix.toml", "line_length=["),
        (MigrationSource::Dprint, "dprint.json", "["),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(name);
        fs::write(&source, body).unwrap();
        assert!(
            migrate_configs(&options(&source, Some(tool))).is_err(),
            "{tool:?}: {body}"
        );
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn explicit_shared_sources_without_supported_settings_are_not_defaulted() {
    for (tool, name, body, error) in [
        (
            MigrationSource::Prettier,
            "package.json",
            "{\"name\":\"project\"}",
            true,
        ),
        (
            MigrationSource::Yamlfix,
            "pyproject.toml",
            "[project]\nname='project'\n",
            false,
        ),
        (
            MigrationSource::Dprint,
            "dprint.json",
            "{extends:'https://example.invalid/config.json'}",
            false,
        ),
        (
            MigrationSource::Yamlfix,
            "custom.yaml",
            "line_length: 90",
            false,
        ),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(name);
        fs::write(&source, body).unwrap();
        let result = migrate_configs(&options(&source, Some(tool)));
        if error {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().entries.is_empty());
        }
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[cfg(unix)]
#[test]
fn discovery_ignores_non_unicode_filenames_and_reports_unreadable_formatter_sources() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let unknown = dir.path().join(OsString::from_vec(b"config-\xff".to_vec()));
    fs::write(unknown, "ignored").unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}\n").unwrap();
    assert_eq!(
        migrate_configs(&options(dir.path(), None))
            .unwrap()
            .entries
            .len(),
        1
    );
    for (tool, name) in [
        (MigrationSource::Prettier, ".prettierrc"),
        (MigrationSource::Yamlfmt, ".yamlfmt"),
    ] {
        let source = dir.path().join(name);
        fs::write(&source, "{}\n").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o000)).unwrap();
        let result = migrate_configs(&options(&source, Some(tool)));
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(result.unwrap_err().contains("failed to read"));
    }
}

#[test]
fn prettier_imports_ignore_patterns_but_warns_about_editorconfig() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".prettierrc"), "tabWidth: 4\n").unwrap();
    fs::write(dir.path().join(".editorconfig"), "[*]\nindent_size=6\n").unwrap();
    fs::write(dir.path().join(".prettierignore"), "ignored.yaml\n[\n").unwrap();
    let result = migrate_configs(&options(dir.path(), None)).unwrap();
    assert_eq!(ryl::format::indent_width(&converted(&result)), 4);
    assert!(
        converted(&result)
            .is_file_ignored(&dir.path().join("ignored.yaml"), dir.path())
    );
    for marker in ["EditorConfig", ".prettierignore:2"] {
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains(marker))
        );
        assert!(result.entries[0].toml.contains(marker));
    }
}

#[test]
fn combined_yamlfix_sources_clean_up_once_after_their_merged_target_is_written() {
    for cleanup in [
        SourceCleanup::Delete,
        SourceCleanup::RenameSuffix(".bak".to_owned()),
    ] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("yamlfix.toml"), "line_length=90\n").unwrap();
        fs::write(dir.path().join(".yamlfix.toml"), "line_length=100\n").unwrap();
        let mut opts = options(dir.path(), Some(MigrationSource::Yamlfix));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = cleanup.clone();
        let result = migrate_configs(&opts).unwrap();
        assert_eq!(ryl::format::line_length(&converted(&result)), 100);
        for name in ["yamlfix.toml", ".yamlfix.toml"] {
            assert!(!dir.path().join(name).exists());
            assert_eq!(
                dir.path().join(format!("{name}.bak")).exists(),
                matches!(cleanup, SourceCleanup::RenameSuffix(_))
            );
        }
    }
}

#[test]
fn yamlfix_ancestor_tables_merge_without_deleting_sources_outside_the_selected_root() {
    let dir = tempdir().unwrap();
    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname='kept'\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("yamlfix.toml"),
        "line_length=90\n[unknown]\na=1\n",
    )
    .unwrap();
    fs::write(
        child.join(".yamlfix.toml"),
        "indent_mapping=4\n[unknown]\nb=2\n",
    )
    .unwrap();
    let mut opts = options(&child, Some(MigrationSource::Yamlfix));
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    let result = migrate_configs(&opts).unwrap();
    assert_eq!(ryl::format::line_length(&converted(&result)), 90);
    let warning = result
        .warnings
        .iter()
        .find(|warning| warning.contains("option unknown"))
        .unwrap();
    assert!(warning.contains("a") && warning.contains("b"), "{warning}");
    assert!(dir.path().join("yamlfix.toml").exists());
    assert!(child.join(".yamlfix.toml").exists());
}

#[test]
fn invalid_native_schema_refuses_a_formatter_import_without_overwriting_sources() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.ryl]\nyaml-files=['*.yaml']\n",
    )
    .unwrap();
    let source = dir.path().join(".prettierrc");
    fs::write(&source, "{}\n").unwrap();
    let error = migrate_configs(&options(dir.path(), None)).unwrap_err();
    assert!(error.contains("yaml-files"), "{error}");
    assert_eq!(fs::read_to_string(source).unwrap(), "{}\n");
    assert!(!dir.path().join(".ryl.toml").exists());
}

#[test]
fn unsupported_dynamic_formatter_preserves_colocated_lint_conversion() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
    fs::write(
        dir.path().join(".prettierrc.js"),
        "throw new Error('never execute')",
    )
    .unwrap();
    let mut opts = options(dir.path(), None);
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    let result = migrate_configs(&opts).unwrap();
    assert!(converted(&result).rule_level("truthy").is_some());
    assert!(!dir.path().join(".yamllint").exists());
    assert!(dir.path().join(".prettierrc.js").exists());
    assert!(result.entries[0].toml.contains("not executed"));
}

#[cfg(unix)]
#[test]
fn formatter_target_and_colocated_lint_symlinks_refuse_the_whole_directory() {
    use std::os::unix::fs::symlink;

    for lint in [false, true] {
        let dir = tempdir().unwrap();
        let sentinel = dir.path().join("sentinel");
        fs::write(&sentinel, "rules: {truthy: enable}\n").unwrap();
        symlink(
            &sentinel,
            dir.path()
                .join(if lint { ".yamllint" } else { ".ryl.toml" }),
        )
        .unwrap();
        fs::write(dir.path().join(".prettierrc"), "{}\n").unwrap();
        let result = migrate_configs(&options(dir.path(), None)).unwrap();
        assert!(result.entries.is_empty());
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("symlink"))
        );
        assert_eq!(
            fs::read_to_string(sentinel).unwrap(),
            "rules: {truthy: enable}\n"
        );
    }
}

#[test]
fn formatter_only_child_never_shadows_a_planned_parent_lint_policy() {
    let dir = tempdir().unwrap();
    let parent = dir.path().join("project");
    let child = parent.join("child");
    let independent = dir.path().join("independent");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(&independent).unwrap();
    fs::write(parent.join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
    let source = child.join(".prettierrc");
    fs::write(&source, "{}\n").unwrap();
    fs::write(independent.join(".prettierrc"), "{}\n").unwrap();
    let input = child.join("input.yaml");
    fs::write(&input, "flag: yes\n").unwrap();
    let (before, _, stderr) = run(ryl(dir.path()).arg("check").arg(&input));
    assert_eq!(before, 1, "{stderr}");
    let mut opts = options(dir.path(), None);
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    let result = migrate_configs(&opts).unwrap();
    assert_eq!(result.entries.len(), 2, "{result:?}");
    assert!(parent.join(".ryl.toml").exists());
    assert!(independent.join(".ryl.toml").exists());
    assert!(!child.join(".ryl.toml").exists());
    assert_eq!(fs::read_to_string(source).unwrap(), "{}\n");
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("shadow inherited configuration")
                && warning.contains("manually"))
    );
    let (after, _, stderr) = run(ryl(dir.path()).arg("check").arg(&input));
    assert_eq!(after, 1, "{stderr}");
    assert!(stderr.contains("truthy"), "{stderr}");
}

#[test]
fn formatter_only_child_retains_sources_under_existing_or_unmigrated_ancestor_policies()
{
    for native in [false, true] {
        let dir = tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(
            dir.path()
                .join(if native { ".ryl.toml" } else { ".yamllint" }),
            if native {
                "[lint.rules]\ntruthy='enable'\n"
            } else {
                "rules: {truthy: enable}\n"
            },
        )
        .unwrap();
        fs::write(child.join(".prettierrc"), "{}\n").unwrap();
        let mut opts = options(&child, None);
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        let result = migrate_configs(&opts).unwrap();
        assert!(result.entries.is_empty(), "{result:?}");
        assert!(child.join(".prettierrc").exists());
        assert!(!child.join(".ryl.toml").exists());
        assert!(!result.warnings.is_empty());
    }
}

#[test]
fn nested_formatter_configs_preserve_independent_defaults_with_or_without_colocated_lint()
 {
    for lint in [false, true] {
        let dir = tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        for source in [dir.path().join(".prettierrc"), child.join(".prettierrc")] {
            fs::write(source, "{}\n").unwrap();
        }
        if lint {
            fs::write(child.join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
        }
        let result = migrate_configs(&options(dir.path(), None)).unwrap();
        assert_eq!(result.entries.len(), 2, "{result:?}");
        if lint {
            assert!(
                result
                    .entries
                    .iter()
                    .any(|entry| entry.target == child.join(".ryl.toml"))
            );
        }
    }
}

#[test]
fn inherited_policy_search_stops_at_the_configured_home_boundary() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let child = home.join("child");
    fs::create_dir_all(&child).unwrap();
    fs::write(dir.path().join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
    fs::write(child.join(".prettierrc"), "{}\n").unwrap();
    let (code, _, stderr) = run(ryl(&home)
        .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
        .arg(&child));
    assert_eq!(code, 0, "{stderr}");
    assert!(child.join(".ryl.toml").exists());
    assert!(!stderr.contains("shadow inherited"));
}

#[test]
fn empty_yamlfmt_and_single_line_dprint_use_their_effective_policies() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".yamlfmt");
    fs::write(&source, "").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
    assert_eq!(
        converted(&result).format().targets().prose_wrap,
        Some(ProseWrap::Never)
    );
    let source = dir.path().join("dprint.json");
    fs::write(&source, "{yaml:{preferSingleLine:true}}").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Dprint))).unwrap();
    let config = converted(&result);
    assert_eq!(
        config.format().targets().sequence_expand,
        ExpandTarget::Auto
    );
    assert_eq!(config.format().targets().mapping_expand, ExpandTarget::Auto);
    fs::write(&source, "[]").unwrap();
    assert!(migrate_configs(&options(&source, Some(MigrationSource::Dprint))).is_err());
}

#[test]
fn inherited_yamlfix_parse_and_section_type_errors_leave_all_sources_intact() {
    for (name, body) in [
        ("yamlfix.toml", "line_length=["),
        ("pyproject.toml", "[tool]\nyamlfix=1\n"),
    ] {
        let dir = tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(dir.path().join(name), body).unwrap();
        fs::write(child.join(".yamlfix.toml"), "line_length=90\n").unwrap();
        let mut opts = options(&child, Some(MigrationSource::Yamlfix));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        assert!(migrate_configs(&opts).is_err());
        assert_eq!(fs::read_to_string(dir.path().join(name)).unwrap(), body);
        assert!(child.join(".yamlfix.toml").exists());
        assert!(!child.join(".ryl.toml").exists());
    }
}

#[test]
fn colocated_lint_errors_refuse_all_writes_and_unrepresentable_format_dimensions_do_not_override_lint()
 {
    let dir = tempdir().unwrap();
    let lint = dir.path().join(".yamllint");
    fs::write(&lint, "rules: [").unwrap();
    fs::write(
        dir.path().join(".prettierrc"),
        "tabWidth: 0\nprintWidth: 0\n",
    )
    .unwrap();
    assert!(migrate_configs(&options(dir.path(), None)).is_err());
    assert!(!dir.path().join(".ryl.toml").exists());
    fs::write(
        &lint,
        "rules: {indentation: {spaces: 4}, line-length: {max: 70}}\n",
    )
    .unwrap();
    let result = migrate_configs(&options(dir.path(), None)).unwrap();
    let config = converted(&result);
    assert_eq!(config.rule_option_int("indentation", "spaces", 0), 4);
    assert_eq!(config.rule_option_int("line-length", "max", 0), 70);
    assert_eq!(ryl::format::indent_width(&config), 2);
    assert_eq!(ryl::format::line_length(&config), 80);
}

#[test]
fn yamlfmt_zero_array_indent_follows_indent_without_a_false_gap_warning() {
    for option in ["", "array_indent: 0", "array_indent: 4"] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        fs::write(&source, format!("formatter:\n  indent: 4\n  {option}\n")).unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
        assert_eq!(ryl::format::indent_width(&converted(&result)), 4);
        assert!(
            !result
                .warnings
                .iter()
                .any(|warning| warning.contains("array_indent")),
            "{result:?}"
        );
    }
}

#[test]
fn yamlfmt_merge_directives_require_manual_expansion_without_writing_misleading_defaults()
 {
    for key in ["<<", "!!merge '<<'", "!<tag:yaml.org,2002:merge> '<<'"] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        let body = format!("formatter:\n  {key}: &base {{indent: 4}}\n");
        fs::write(&source, &body).unwrap();
        let mut opts = options(&source, Some(MigrationSource::Yamlfmt));
        opts.write_mode = WriteMode::Write;
        opts.cleanup = SourceCleanup::Delete;
        let result = migrate_configs(&opts).unwrap();
        assert!(result.entries.is_empty(), "{key}: {result:?}");
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("expand merged settings explicitly"))
        );
        assert_eq!(fs::read_to_string(source).unwrap(), body);
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}

#[test]
fn yamlfmt_quoted_and_string_tagged_merge_lookalikes_remain_ordinary_unknown_keys() {
    for (key, unknown) in [
        ("'<<'", "<<"),
        ("!!str <<", "<<"),
        ("!!merge other", "other"),
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        fs::write(
            &source,
            format!("formatter:\n  {key}: {{indent: 4}}\n  unknown: <<\n"),
        )
        .unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
        assert_eq!(ryl::format::indent_width(&converted(&result)), 2);
        assert!(
            !result
                .warnings
                .iter()
                .any(|warning| warning.contains("expand merged"))
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains(&format!("option {unknown}")))
        );
    }
}

#[test]
fn prettier_yaml_merge_spelling_stays_literal_instead_of_inheriting_yamlfmt_semantics()
{
    let dir = tempdir().unwrap();
    let source = dir.path().join(".prettierrc.yaml");
    fs::write(&source, "tabWidth: 3\n<<: &base {tabWidth: 4}\n").unwrap();
    let result =
        migrate_configs(&options(&source, Some(MigrationSource::Prettier))).unwrap();
    assert_eq!(ryl::format::indent_width(&converted(&result)), 3);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("option <<"))
    );
}

#[test]
fn yamlfmt_recognized_nulls_and_empty_identity_fields_use_source_defaults() {
    for body in [
        "formatter: null\n",
        "line_ending: ''\nformatter: {type: ''}\n",
        "line_ending: null\nformatter: {type: null, indent: null, max_line_length: null, force_quote_style: null, include_document_start: null, retain_line_breaks: null, retain_line_breaks_single: null, array_indent: null, indent_root_array: null, indentless_arrays: null, force_array_style: null, pad_line_comments: null, scan_folded_as_literal: null, unknown: null}\n",
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join(".yamlfmt");
        fs::write(&source, body).unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
        let config = converted(&result);
        assert_eq!(ryl::format::indent_width(&config), 2);
        assert_eq!(
            config.format().targets().line_ending,
            LineEndingTarget::Native
        );
        assert_eq!(config.format().targets().comment_spacing.get(), 1);
        assert_eq!(config.format().targets().prose_wrap, Some(ProseWrap::Never));
        if body.contains("unknown") {
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("option unknown"))
            );
        }
    }
}

#[test]
fn explicit_formatter_files_merge_colocated_lint_without_cleaning_outside_the_selected_file()
 {
    for (tool, filename, body) in [
        (MigrationSource::Prettier, ".prettierrc.json", "{}"),
        (MigrationSource::Yamlfmt, ".yamlfmt", "{}"),
        (MigrationSource::Dprint, "dprint.json", DPRINT),
        (MigrationSource::Yamlfix, "yamlfix.toml", "line_length=90\n"),
    ] {
        for cleanup in [
            SourceCleanup::Keep,
            SourceCleanup::Delete,
            SourceCleanup::RenameSuffix(".bak".to_owned()),
        ] {
            let dir = tempdir().unwrap();
            let lint = dir.path().join(".yamllint");
            let lower = dir.path().join(".yamllint.yml");
            fs::write(&lint, "rules: {truthy: enable}\n").unwrap();
            fs::write(&lower, "rules: {}\n").unwrap();
            fs::write(dir.path().join(".yamllint.bak"), "old backup\n").unwrap();
            let source = dir.path().join(filename);
            fs::write(&source, body).unwrap();
            let input = dir.path().join("input.yaml");
            fs::write(&input, "flag: yes\n").unwrap();
            let (before, _, stderr) = run(ryl(dir.path()).arg("check").arg(&input));
            assert_eq!(before, 1, "{stderr}");
            let mut opts = options(&source, Some(tool));
            opts.write_mode = WriteMode::Write;
            opts.cleanup = cleanup;
            let result = migrate_configs(&opts).unwrap();
            assert_eq!(result.entries.len(), 1);
            assert!(converted(&result).rule_level("truthy").is_some());
            assert_eq!(
                fs::read_to_string(&lint).unwrap(),
                "rules: {truthy: enable}\n"
            );
            assert_eq!(fs::read_to_string(&lower).unwrap(), "rules: {}\n");
            assert_eq!(
                fs::read_to_string(dir.path().join(".yamllint.bak")).unwrap(),
                "old backup\n"
            );
            let (after, _, stderr) = run(ryl(dir.path()).arg("check").arg(&input));
            assert_eq!(after, 1, "{tool:?}: {stderr}");
        }
    }
}

#[test]
fn explicit_unsupported_formatter_files_do_not_migrate_unselected_colocated_lint() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".yamllint"), "rules: {truthy: enable}\n").unwrap();
    let source = dir.path().join(".prettierrc.js");
    fs::write(&source, "throw new Error('never execute')").unwrap();
    let mut opts = options(&source, Some(MigrationSource::Prettier));
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    let result = migrate_configs(&opts).unwrap();
    assert!(result.entries.is_empty());
    assert!(result.cleanup_only_sources.is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("not executed"))
    );
    assert!(dir.path().join(".yamllint").exists());
    assert!(source.exists());
    assert!(!dir.path().join(".ryl.toml").exists());
}

#[test]
fn cleanup_resolves_all_dotdot_root_sources_before_deleting_or_renaming_any() {
    for cleanup in [
        SourceCleanup::Delete,
        SourceCleanup::RenameSuffix(".bak".to_owned()),
    ] {
        let dir = tempdir().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::create_dir(dir.path().join("nested")).unwrap();
        fs::write(dir.path().join("yamlfix.toml"), "line_length=90\n").unwrap();
        fs::write(child.join(".yamlfix.toml"), "indent_mapping=4\n").unwrap();
        let mut opts = options(
            &dir.path().join("nested/.."),
            Some(MigrationSource::Yamlfix),
        );
        opts.write_mode = WriteMode::Write;
        opts.cleanup = cleanup.clone();
        let result = migrate_configs(&opts).unwrap();
        assert_eq!(result.entries.len(), 2);
        for source in [dir.path().join("yamlfix.toml"), child.join(".yamlfix.toml")] {
            assert!(!source.exists());
            assert_eq!(
                source
                    .with_file_name(format!(
                        "{}.bak",
                        source.file_name().unwrap().to_string_lossy()
                    ))
                    .exists(),
                matches!(cleanup, SourceCleanup::RenameSuffix(_))
            );
        }
        assert!(dir.path().join(".ryl.toml").exists());
        assert!(child.join(".ryl.toml").exists());
    }
}

#[test]
fn yamlfmt_timestamp_resolution_never_becomes_a_string_cleanup_candidate() {
    for value in [
        "2000-01-01",
        "2000-1-1",
        "2000-01-1",
        "2000-1-01",
        "2000-01-01T00:00:00Z",
        "2000-1-1t1:2:3Z",
        "2000-1-1 1:2:3",
        "2000-1-1t1:2:3,4Z",
    ] {
        for body in [
            format!("include:\n  - {value}\n"),
            format!("gitignore_path: {value}\n"),
        ] {
            let dir = tempdir().unwrap();
            let source = dir.path().join(".yamlfmt");
            fs::write(&source, &body).unwrap();
            let mut opts = options(&source, Some(MigrationSource::Yamlfmt));
            opts.write_mode = WriteMode::Write;
            opts.cleanup = SourceCleanup::Delete;
            let result = migrate_configs(&opts).unwrap();
            assert!(result.entries.is_empty());
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("quote string values"))
            );
            assert_eq!(fs::read_to_string(&source).unwrap(), body);
            assert!(!dir.path().join(".ryl.toml").exists());
            fs::write(&source, format!("include: ['{value}']\n")).unwrap();
            assert_eq!(migrate_configs(&opts).unwrap().entries.len(), 1);
            assert!(!source.exists());
        }
    }
}

#[test]
fn source_timestamp_uncertainty_keeps_suffixed_paths_and_plain_string_controls() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".yamlfmt");
    for value in [
        "2000-backups.yaml",
        "2000-01-01.schema.yaml",
        "2000-01",
        "1:20",
    ] {
        fs::write(&source, format!("include: [{value}]\n")).unwrap();
        assert_eq!(
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt)))
                .unwrap()
                .entries
                .len(),
            1
        );
    }
}

#[test]
fn normalization_rejects_unsupported_numeric_keys_before_writing_a_target() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".yamlfmt");
    let body = "formatter: {indent: 2}\n010: first\n8: second\n";
    fs::write(&source, body).unwrap();
    let mut opts = options(&source, Some(MigrationSource::Yamlfmt));
    opts.write_mode = WriteMode::Write;
    opts.cleanup = SourceCleanup::Delete;
    assert!(migrate_configs(&opts).is_err());
    assert_eq!(fs::read_to_string(source).unwrap(), body);
    assert!(!dir.path().join(".ryl.toml").exists());
}

#[test]
fn yaml_only_prettier_overrides_apply_in_source_order() {
    for (files, applied) in [
        (r#""*.yaml""#, true),
        (r#""src/**/*.yaml""#, true),
        (r#""[abc]*.yml""#, true),
        (r#"["**/*.{yaml,yml}", "*.yml"]"#, true),
        (r#""*.md""#, false),
        (r#""*""#, false),
        (r#""*.{yaml,md}""#, false),
        (r#"["*.yaml", "*.md"]"#, false),
        (r#""!*.yaml""#, false),
        (r#""*.{yaml}""#, false),
        (r#""*.yaml,yml""#, false),
        (r#""*.{yaml,,yml}""#, false),
        (r#""**/{src,test}/*.yaml""#, false),
        (r#"[]"#, false),
    ] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".prettierrc"), format!(r#"{{"proseWrap":"always","overrides":[{{"files":{files},"options":{{"proseWrap":"never"}}}},{{"files":{files},"options":{{"proseWrap":"preserve"}}}}]}}"#)).unwrap();
        let result =
            migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier)))
                .unwrap();
        let table: toml::Value = toml::from_str(&result.entries[0].toml).unwrap();
        assert_eq!(
            table["format"]["prose-wrap"].as_str(),
            Some(if applied { "preserve" } else { "always" })
        );
        assert_eq!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("option overrides")),
            !applied,
            "{result:?}"
        );
    }
}

#[test]
fn imported_exclusions_preserve_source_patterns_and_negations() {
    for (tool, filename, body, ignore) in [
        (
            MigrationSource::Prettier,
            ".prettierrc",
            "{}",
            Some("# comment\n*.yaml\n!keep.yaml\n"),
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            "match_type: doublestar\nexclude: ['skip.yaml', '!keep.yaml']\n",
            None,
        ),
        (
            MigrationSource::Dprint,
            "dprint.json",
            r#"{"yaml":{},"excludes":["*.yaml","!keep.yaml"]}"#,
            None,
        ),
    ] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(filename), body).unwrap();
        if let Some(ignore) = ignore {
            fs::write(dir.path().join(".prettierignore"), ignore).unwrap();
        }
        let result = migrate_configs(&options(dir.path(), Some(tool))).unwrap();
        let config = converted(&result);
        assert!(
            config.is_file_ignored(&dir.path().join("skip.yaml"), dir.path()),
            "{result:?}"
        );
        assert!(
            !config.is_file_ignored(&dir.path().join("keep.yaml"), dir.path()),
            "{result:?}"
        );
        assert_eq!(
            config.is_file_ignored(&dir.path().join("sub/skip.yaml"), dir.path()),
            tool != MigrationSource::Yamlfmt
        );
    }
}

#[test]
fn import_reports_the_formatter_conflicts_with_colocated_lint() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".yamllint"), "extends: default\n").unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    let result =
        migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier))).unwrap();
    let conflicts = ryl::format::conflicts(&converted(&result));
    assert!(!conflicts.is_empty());
    for conflict in conflicts {
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains(&conflict)),
            "{result:?}"
        );
    }
}

#[test]
fn dprint_local_extends_merges_first_base_precedence_and_exclusions() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("first.json"),
        r#"{"yaml":{"printWidth":40},"excludes":["first.yaml"]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("second.json"),
        r#"{"yaml":{"printWidth":60,"indentWidth":4},"excludes":["second.yaml"]}"#,
    )
    .unwrap();
    fs::write(dir.path().join("dprint.json"), r#"{"extends":["first.json","second.json"],"yaml":{"indentWidth":3},"excludes":["own.yaml"]}"#).unwrap();
    let result =
        migrate_configs(&options(dir.path(), Some(MigrationSource::Dprint))).unwrap();
    let config = converted(&result);
    assert_eq!(ryl::format::line_length(&config), 40);
    assert_eq!(ryl::format::indent_width(&config), 3);
    for name in ["first.yaml", "second.yaml", "own.yaml"] {
        assert!(config.is_file_ignored(&dir.path().join(name), dir.path()));
    }
}

#[test]
fn unresolvable_dprint_extends_skips_the_entire_source() {
    for extends in [
        "missing.json",
        "dprint.json",
        "https://example.invalid/base.json",
    ] {
        let dir = tempdir().unwrap();
        let source = dir.path().join("dprint.json");
        fs::write(
            &source,
            format!(r#"{{"extends":"{extends}","yaml":{{"printWidth":100}}}}"#),
        )
        .unwrap();
        let mut opts = options(dir.path(), Some(MigrationSource::Dprint));
        opts.write_mode = WriteMode::Write;
        let result = migrate_configs(&opts).unwrap();
        assert!(result.entries.is_empty(), "{result:?}");
        assert_eq!(result.skipped, 1);
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("flatten"))
        );
        assert!(!dir.path().join(".ryl.toml").exists());
        assert!(source.exists());
    }
}

#[test]
fn dprint_extends_bounds_depth_and_repeated_base_expansion() {
    for repeated in [false, true] {
        let dir = tempdir().unwrap();
        for index in 0..33 {
            fs::write(
                dir.path().join(format!("base{index}.json")),
                if repeated {
                    "{\"yaml\":{}}".to_owned()
                } else {
                    format!(r#"{{"extends":"base{}.json","yaml":{{}}}}"#, index + 1)
                },
            )
            .unwrap();
        }
        let bases = if repeated {
            vec!["base0.json"; 257]
        } else {
            vec!["base0.json"]
        };
        fs::write(
            dir.path().join("dprint.json"),
            serde_json::json!({"extends":bases,"yaml":{}}).to_string(),
        )
        .unwrap();
        let result =
            migrate_configs(&options(dir.path(), Some(MigrationSource::Dprint)))
                .unwrap();
        assert_eq!(result.skipped, 1);
        assert!(result.entries.is_empty());
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("excessive")),
            "{result:?}"
        );
    }
}

#[test]
fn inherited_dprint_values_reject_invalid_extends_and_non_objects() {
    for body in [r#"{"extends":true}"#, r#"{"extends":[42]}"#, "[]"] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("base.json"), body).unwrap();
        fs::write(
            dir.path().join("dprint.json"),
            r#"{"extends":"base.json","yaml":{}}"#,
        )
        .unwrap();
        let result =
            migrate_configs(&options(dir.path(), Some(MigrationSource::Dprint)))
                .unwrap();
        assert_eq!(result.skipped, 1, "{result:?}");
        assert!(result.entries.is_empty());
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("flatten"))
        );
    }
}

#[cfg(unix)]
#[test]
fn inherited_dprint_symlinks_are_retained_without_importing_settings() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("real.json"),
        r#"{"yaml":{"printWidth":100}}"#,
    )
    .unwrap();
    symlink(dir.path().join("real.json"), dir.path().join("base.json")).unwrap();
    fs::write(dir.path().join("dprint.json"), r#"{"extends":"base.json"}"#).unwrap();
    let result =
        migrate_configs(&options(dir.path(), Some(MigrationSource::Dprint))).unwrap();
    assert_eq!(result.skipped, 1);
    assert!(result.entries.is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("symlink"))
    );
    assert!(dir.path().join("base.json").is_symlink());
}

#[cfg(unix)]
#[test]
fn prettier_ignore_symlinks_warn_and_unreadable_files_fail_import() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".prettierrc"), "{}").unwrap();
    let source = dir.path().join("patterns");
    fs::write(&source, "ignored.yaml\n").unwrap();
    let ignore = dir.path().join(".prettierignore");
    symlink(&source, &ignore).unwrap();
    let result =
        migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier))).unwrap();
    assert!(
        !converted(&result)
            .is_file_ignored(&dir.path().join("ignored.yaml"), dir.path())
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains(".prettierignore is a symlink"))
    );
    fs::remove_file(&ignore).unwrap();
    fs::write(&ignore, "ignored.yaml\n").unwrap();
    fs::set_permissions(&ignore, fs::Permissions::from_mode(0o000)).unwrap();
    let result = migrate_configs(&options(dir.path(), Some(MigrationSource::Prettier)));
    fs::set_permissions(&ignore, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(result.unwrap_err().contains("cannot read"));
}

#[test]
fn yamlfmt_exclusion_path_spellings_are_normalized_or_warned() {
    let dir = tempdir().unwrap();
    let source = dir.path().join(".yamlfmt");
    fs::create_dir(dir.path().join("nested")).unwrap();
    for (pattern, expected) in [
        ("skip.yaml".to_owned(), Some("/skip.yaml")),
        ("./skip.yaml".to_owned(), Some("/skip.yaml")),
        ("nested/../skip.yaml".to_owned(), Some("/skip.yaml")),
        ("skip.yaml/".to_owned(), Some("/skip.yaml")),
        (
            dir.path().join("skip.yaml").display().to_string(),
            Some("/skip.yaml"),
        ),
        ("./nested/*.yaml".to_owned(), Some("/nested/*.yaml")),
        ("nested/**".to_owned(), Some("/nested/**")),
        ("../skip.yaml".to_owned(), None),
        (
            dir.path()
                .parent()
                .unwrap()
                .join("skip.yaml")
                .display()
                .to_string(),
            None,
        ),
        ("nested".to_owned(), None),
        ("nested/".to_owned(), None),
        (".".to_owned(), None),
        (String::new(), None),
    ] {
        let body = serde_json::json!({
            "match_type": "doublestar",
            "exclude": [pattern],
            "formatter": {"include_document_start": true},
        });
        fs::write(&source, body.to_string()).unwrap();
        let result =
            migrate_configs(&options(&source, Some(MigrationSource::Yamlfmt))).unwrap();
        let table: toml::Table = toml::from_str(&result.entries[0].toml).unwrap();
        let config = converted(&result);
        if let Some(expected) = expected {
            assert_eq!(
                table["exclude"].as_array().unwrap(),
                &[toml::Value::from(expected)],
                "{pattern}"
            );
            assert!(result.warnings.is_empty(), "{pattern}: {result:?}");
            let ignored = if expected.starts_with("/nested/") {
                "nested/skip.yaml"
            } else {
                "skip.yaml"
            };
            assert!(
                config.is_file_ignored(&dir.path().join(ignored), dir.path()),
                "{pattern}"
            );
            assert!(
                !config
                    .is_file_ignored(&dir.path().join("other/skip.yaml"), dir.path()),
                "{pattern}"
            );
        } else {
            assert!(!table.contains_key("exclude"), "{pattern}");
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("yamlfmt option exclude")
                        && warning.contains("configure exclude manually")),
                "{pattern}: {result:?}"
            );
            assert!(
                !config
                    .is_file_ignored(&dir.path().join("nested/skip.yaml"), dir.path()),
                "{pattern}"
            );
        }
    }
}

#[test]
fn formatter_exclusions_append_to_colocated_lint_exclusions() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".yamllint"),
        "ignore: lint.yaml\nrules: {truthy: enable}\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("dprint.json"),
        r#"{"yaml":{},"excludes":["format.yaml"]}"#,
    )
    .unwrap();
    let result =
        migrate_configs(&options(dir.path(), Some(MigrationSource::Dprint))).unwrap();
    let config = converted(&result);
    for name in ["lint.yaml", "format.yaml"] {
        assert!(config.is_file_ignored(&dir.path().join(name), dir.path()));
    }
}

#[test]
fn yaml_specific_overrides_and_doublestar_exclusions_validate_imported_values() {
    for (tool, filename, body, error) in [
        (
            MigrationSource::Prettier,
            ".prettierrc",
            r#"{"overrides":[{"files":"*.yaml","options":{"overrides":true}}]}"#,
            "option overrides",
        ),
        (
            MigrationSource::Yamlfmt,
            ".yamlfmt",
            "match_type: doublestar\nexclude: true\n",
            "option exclude",
        ),
    ] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(filename), body).unwrap();
        let result = migrate_configs(&options(dir.path(), Some(tool)));
        assert!(result.unwrap_err().contains(error));
        assert!(!dir.path().join(".ryl.toml").exists());
    }
}
