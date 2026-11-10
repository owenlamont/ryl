use std::fs;
use std::path::Path;

use ryl::config::{ConfigContext, Overrides, SourceKind, SystemEnv, discover_config};

use crate::common::cli::{command_output, run, run_with_stdin, ryl};

fn fixture(root: &Path, extra: &str) -> ConfigContext {
    fs::write(root.join("allow.json"), "true").unwrap();
    fs::write(root.join("deny.json"), "false").unwrap();
    fs::write(
        root.join(".ryl.toml"),
        format!("[lint.rules.schema]\ndirectives = true\n{extra}"),
    )
    .unwrap();
    discover_config(
        &[],
        &Overrides {
            config_file: Some(root.join(".ryl.toml")),
            config_data: None,
        },
    )
    .unwrap()
}

fn hits(ctx: &ConfigContext, text: &str) -> Vec<ryl::LintProblem> {
    let path = ctx.base_dir.join("in.yaml");
    let prepared = ctx
        .config
        .document_schemas(
            text,
            Some(&path),
            &ctx.base_dir,
            SourceKind::Yaml,
            &SystemEnv,
        )
        .unwrap();
    ryl::lint_str(text, &path, &prepared, &ctx.base_dir)
}

#[cfg(debug_assertions)]
#[test]
fn inactive_directives_do_not_scan_or_consult_the_environment() {
    use crate::common::fake_env::FakeEnv;
    let input = "# $schema: /missing.json\nx: 1\n";
    for config in [
        "[lint.rules]\nALL = 'enable'\n",
        "[lint.rules.schema]\ndirectives = false\n",
        "[lint.rules]\nschema = 'disable'\n",
    ] {
        let cfg = ryl::config::YamlLintConfig::from_toml_str(config).unwrap();
        let before = ryl::rules::schema::directive_scan_count();
        let result = cfg
            .document_schemas(
                input,
                None,
                Path::new("/missing-base"),
                SourceKind::Yaml,
                &FakeEnv::new(),
            )
            .unwrap();
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(ryl::rules::schema::directive_scan_count(), before);
    }
    let before = ryl::rules::schema::directive_scan_count();
    assert_eq!(
        ryl::rules::schema::document_directives(input),
        vec![Some("/missing.json".to_owned())]
    );
    assert_eq!(ryl::rules::schema::directive_scan_count(), before + 1);
}

#[test]
fn ignored_directives_do_not_prepare_and_invalid_overrides_do_not_fall_back() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(
        dir.path(),
        "[[lint.rules.schema.schemas]]\npath = 'allow.json'\n",
    );
    let input = "# $schema: missing.json\nx: 1\n";
    assert!(
        ctx.config
            .document_schemas(
                input,
                Some(&dir.path().join("in.yaml")),
                &ctx.base_dir,
                SourceKind::Yaml,
                &SystemEnv
            )
            .is_err()
    );
    let ctx = fixture(
        dir.path(),
        "[lint.per-file-ignores]\n'*.yaml' = ['schema']\n",
    );
    let prepared = ctx
        .config
        .document_schemas(
            input,
            Some(&dir.path().join("in.yaml")),
            &ctx.base_dir,
            SourceKind::Yaml,
            &SystemEnv,
        )
        .unwrap();
    assert!(matches!(prepared, std::borrow::Cow::Borrowed(_)));
    assert!(
        hits(&ctx, "# ryl disable-file\n# $schema: missing.json\nx: 1\n").is_empty()
    );
    fs::remove_file(dir.path().join("allow.json")).unwrap();
    fs::write(dir.path().join(".ryl.toml"), "[lint.rules.schema]\ndirectives = true\n[[lint.rules.schema.schemas]]\npath = 'allow.json'\n").unwrap();
    assert!(
        discover_config(
            &[],
            &Overrides {
                config_file: Some(dir.path().join(".ryl.toml")),
                config_data: None
            }
        )
        .is_err()
    );
}

#[test]
fn markdown_regions_select_independently_and_share_diagnostic_and_loading_limits() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(dir.path(), "[files]\nmarkdown = ['*.md']\n");
    let path = dir.path().join("in.md");
    let source =
        "---\n# $schema: none\nx: 1\n---\n\n```yaml\n# $schema: deny.json\nx: 2\n```\n";
    let prepared = ctx
        .config
        .document_schemas(
            source,
            Some(&path),
            &ctx.base_dir,
            SourceKind::Markdown,
            &SystemEnv,
        )
        .unwrap();
    let hits = ryl::lint_markdown_str(source, &path, &prepared, &ctx.base_dir);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 8);
    fs::write(&path, source).unwrap();
    let mut command = ryl(dir.path());
    command
        .args(["check", "--output-format", "parsable"])
        .arg(&path);
    let (status, stdout, stderr) = run(&mut command);
    assert_eq!(status, 1);
    let output = command_output(&stdout, &stderr);
    assert!(output.contains("schema"), "{output}");
    let skipped =
        "```yaml\n# $schema: https://denied.example.org/schema.json\rx: 1\n```\n";
    assert!(
        ctx.config
            .document_schemas(
                skipped,
                Some(&path),
                &ctx.base_dir,
                SourceKind::Markdown,
                &SystemEnv
            )
            .is_ok()
    );
    let source = "```yaml\n# $schema: deny.json\nx: 1\n```\n".repeat(102);
    let prepared = ctx
        .config
        .document_schemas(
            &source,
            Some(&path),
            &ctx.base_dir,
            SourceKind::Markdown,
            &SystemEnv,
        )
        .unwrap();
    let hits = ryl::lint_markdown_str(&source, &path, &prepared, &ctx.base_dir);
    assert_eq!(hits.len(), 101);
    assert!(hits.last().unwrap().message.contains("truncated"));
    let large =
        serde_json::json!({"description":"x".repeat(9 * 1024 * 1024)}).to_string();
    fs::write(dir.path().join("large.json"), &large).unwrap();
    fs::write(dir.path().join("other.json"), &large).unwrap();
    let source = "```yaml\n# $schema: large.json\nx: 1\n```\n```yaml\n# $schema: other.json\nx: 2\n```\n";
    let error = ctx
        .config
        .document_schemas(
            source,
            Some(&path),
            &ctx.base_dir,
            SourceKind::Markdown,
            &SystemEnv,
        )
        .unwrap_err();
    assert!(error.message.contains("size limit"));
}

#[test]
fn configured_and_directive_roots_share_one_resource_budget() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(
        dir.path(),
        "[[lint.rules.schema.schemas]]\npath = 'allow.json'\n",
    );
    let mut source = String::new();
    for index in 0..1024 {
        fs::write(dir.path().join(format!("{index}.json")), "true").unwrap();
        source.push_str(&format!("---\n# $schema: {index}.json\nx: 1\n"));
    }
    let error = ctx
        .config
        .document_schemas(
            &source,
            Some(&dir.path().join("in.yaml")),
            &ctx.base_dir,
            SourceKind::Yaml,
            &SystemEnv,
        )
        .unwrap_err();
    assert!(error.message.contains("resource limit"));
}

#[test]
fn header_matching_follows_red_hat_without_reading_yaml_data() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(dir.path(), "");
    for (comment, selected) in [
        ("# yaml-language-server: $schema=deny.json", true),
        ("# $schema:deny.json", true),
        (
            "#\t yaml-language-server \t: $schema:\tdeny.json trailing",
            true,
        ),
        ("#\u{a0}$schema:\u{feff}deny.json", true),
        ("#\u{2003}yaml-language-server: $schema=deny.json", true),
        ("#yaml-language-server: $schema=deny.json", false),
        ("# YAML-language-server: $schema=deny.json", false),
        ("# $Schema: deny.json", false),
        ("# yaml-language-server: $schema= deny.json", false),
        ("#\u{85}$schema: deny.json", false),
    ] {
        let text = format!("{comment}\nx: 1\n");
        assert_eq!(hits(&ctx, &text).len(), usize::from(selected), "{text:?}");
    }
    for text in [
        "$schema: deny.json\n",
        "x: '# $schema: deny.json'\n",
        "x: |\n  # $schema: deny.json\n",
        "x: 1\n# $schema: deny.json\n",
        "x: 1\n...\n# $schema: missing.json\n",
        "# ryl disable-file\n# $schema: missing.json\nx: 1\n",
    ] {
        assert!(hits(&ctx, text).is_empty(), "{text:?}");
    }
}

#[test]
fn low_level_validation_never_loads_an_unprepared_directive() {
    let cfg = ryl::config::YamlLintConfig::from_toml_str(
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    assert!(
        ryl::rules::schema::check(
            "# $schema: /missing.json\nx: 1\n",
            &cfg,
            Path::new("in.yaml")
        )
        .is_empty()
    );
}

#[test]
fn root_fragments_keep_their_dialect_without_overwriting_schema_ids() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(dir.path(), "");
    fs::write(dir.path().join("fragment.json"), r##"{"$schema":"http://json-schema.org/draft-04/schema#","id":"urn:ryl:schema-selection:0","definitions":{"leaf":{"$ref":"#/definitions/number"},"number":{"type":"number","minimum":3,"exclusiveMinimum":true}}}"##).unwrap();
    assert_eq!(
        hits(&ctx, "# $schema: fragment.json#/definitions/leaf\n3\n").len(),
        1
    );
    assert!(hits(&ctx, "# $schema: fragment.json#/definitions/leaf\n4\n").is_empty());
}

#[test]
fn key_sorting_cannot_promote_an_inactive_modeline_into_the_header() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(dir.path(), "[lint.rules.key-ordering]\n");
    let source = "z: 1\n# $schema: none\na: 2\n";
    assert_eq!(
        ryl::fix::apply_safe_fixes(
            source,
            &ctx.config,
            &dir.path().join("in.yaml"),
            &ctx.base_dir
        ),
        source
    );
    assert_eq!(ryl::rules::schema::document_directives(source), vec![None]);
}

#[test]
fn cli_preparation_failures_exit_two_in_check_fix_diff_and_stdin() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), "[lint.rules.colons]\n");
    let path = dir.path().join("in.yaml");
    let source = "# $schema: missing.json\nx:  1\n";
    fs::write(&path, source).unwrap();
    for flags in [vec![], vec!["--fix"], vec!["--diff"], vec!["--no-cache"]] {
        let (code, _, stderr) =
            run(ryl(dir.path()).arg("check").args(&flags).arg(&path));
        assert_eq!(code, 2, "{flags:?}: {stderr}");
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
    }
    let mut command = ryl(dir.path());
    command
        .args(["check", "--diff", "-", "-c"])
        .arg(dir.path().join(".ryl.toml"));
    assert_eq!(run_with_stdin(&mut command, source.as_bytes()).0, 2);
    let missing = dir.path().join("missing.yaml");
    assert_eq!(run(ryl(dir.path()).arg("check").arg(&missing)).0, 1);
    fs::write(&path, [0xff, 0xff, 0xfe]).unwrap();
    assert_eq!(run(ryl(dir.path()).arg("check").arg(&path)).0, 1);
    fs::write(&path, "# $schema: allow.json\nx: 1\n").unwrap();
    let cache = dir.path().join("cache-file");
    fs::write(&cache, "occupied").unwrap();
    assert_eq!(
        run(ryl(dir.path())
            .args(["check", "--cache-dir"])
            .arg(&cache)
            .arg(&path))
        .0,
        2
    );
}

#[cfg(unix)]
#[test]
fn fix_checks_symlink_inputs_without_loading_a_refused_directive() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), "");
    let path = dir.path().join("in.yaml");
    let alias = dir.path().join("alias.yaml");
    fs::write(&path, "# $schema: missing.json\nx: 1\n").unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert_eq!(
        run(ryl(dir.path()).args(["check", "--fix"]).arg(&alias)).0,
        2
    );
}

#[test]
fn first_modeline_and_each_document_own_their_selection() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(
        dir.path(),
        "[[lint.rules.schema.schemas]]\npath = 'deny.json'\n",
    );
    for (text, count) in [
        ("# $schema: NoNe\nx: 1\n", 0),
        ("# yaml-language-server:\n# $schema: none\nx: 1\n", 1),
        ("# $schema: allow.json\n# $schema: deny.json\nx: 1\n", 0),
        ("# $schema: none\n---\nx: 1\n---\nx: 2\n", 1),
        ("x: 1\n# $schema: none\n---\nx: 2\n", 2),
        ("x: 1\n...\n# $schema: none\n---\nx: 2\n", 1),
        ("x: 1\n... # $schema: none\n---\nx: 2\n", 2),
        ("--- # $schema: none\nx: 1\n", 0),
        ("# $schema: none\n---\n---\n# $schema: none\nx: 2\n", 0),
        ("%YAML 1.2\n# $schema: none\n---\nx: 1\n", 0),
    ] {
        assert_eq!(hits(&ctx, text).len(), count, "{text:?}");
    }
    for ending in ["\n", "\r\n", "\r"] {
        assert!(
            hits(
                &ctx,
                &format!("\u{feff}# $schema: none{ending}x: 1{ending}")
            )
            .is_empty()
        );
    }
}

#[test]
fn local_directives_resolve_beside_input_but_remain_inside_config_root() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(dir.path(), "");
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("literal%20.json"), "true").unwrap();
    fs::write(dir.path().join("fragment.json"), r##"{"$defs":{"reject":{"$anchor":"reject","type":"string"}},"type":"integer"}"##).unwrap();
    let path = dir.path().join("sub/in.yaml");
    for value in [
        "../allow.json",
        "../literal%20.json",
        "../fragment.json#/$defs/reject",
        "../fragment.json#reject",
    ] {
        let text = format!("# $schema: {value}\nhello\n");
        let prepared = ctx
            .config
            .document_schemas(
                &text,
                Some(&path),
                &ctx.base_dir,
                SourceKind::Yaml,
                &SystemEnv,
            )
            .unwrap();
        assert!(
            ryl::lint_str(&text, &path, &prepared, &ctx.base_dir).is_empty(),
            "{value}"
        );
    }
    for value in [
        "../../outside.json",
        "/etc/passwd",
        "C:\\schema.json",
        "\\\\host\\schema.json",
        "//host/schema.json",
        "file:///etc/passwd",
        "http://example.org/schema.json",
        "missing.json",
        "../allow.json#%",
        "../allow.json#/absent",
        "\"a b.json\"",
    ] {
        let text = format!("# $schema: {value}\nx: 1\n");
        let error = ctx
            .config
            .document_schemas(
                &text,
                Some(&path),
                &ctx.base_dir,
                SourceKind::Yaml,
                &SystemEnv,
            )
            .unwrap_err();
        assert_eq!((error.line, error.column), (1, 1), "{value}");
    }
    let error = ctx
        .config
        .document_schemas(
            "# $schema: allow.json\nx: 1\n",
            None,
            &ctx.base_dir,
            SourceKind::Yaml,
            &SystemEnv,
        )
        .unwrap_err();
    assert!(error.message.contains("--stdin-filename"));
}

#[cfg(unix)]
#[test]
fn symlink_directive_cannot_escape_the_config_root() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let ctx = fixture(dir.path(), "");
    fs::write(outside.path().join("schema.json"), "true").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
    let error = ctx
        .config
        .document_schemas(
            "# $schema: escape/schema.json\nx: 1\n",
            Some(&dir.path().join("in.yaml")),
            &ctx.base_dir,
            SourceKind::Yaml,
            &SystemEnv,
        )
        .unwrap_err();
    assert!(error.message.contains("escapes config base"));
}

#[test]
fn cli_directive_cache_tracks_root_and_transitive_edits() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), "");
    fs::write(dir.path().join("root.json"), r#"{"$ref":"allow.json"}"#).unwrap();
    let path = dir.path().join("in.yaml");
    fs::write(&path, "# $schema: root.json\nx: 1\n").unwrap();
    let check = || {
        let mut command = ryl(dir.path());
        command
            .arg("check")
            .arg("--cache-dir")
            .arg(dir.path().join("cache"))
            .arg(&path);
        run(&mut command)
    };
    assert_eq!(check().0, 0);
    assert_eq!(check().0, 0);
    fs::write(dir.path().join("allow.json"), "false").unwrap();
    assert_eq!(check().0, 1);
    fs::write(dir.path().join("root.json"), "true").unwrap();
    assert_eq!(check().0, 0);
    let mut command = ryl(dir.path());
    command
        .args(["check", "-", "-c"])
        .arg(dir.path().join(".ryl.toml"));
    assert_eq!(
        run_with_stdin(&mut command, b"# $schema: allow.json\nx: 1\n").0,
        2
    );
    let mut command = ryl(dir.path());
    command.args(["check", "-", "--stdin-filename"]).arg(&path);
    assert_eq!(
        run_with_stdin(&mut command, b"# $schema: allow.json\nx: 1\n").0,
        1
    );
}

#[test]
fn rewrites_preserve_active_inactive_and_malformed_modelines() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = fixture(
        dir.path(),
        "[lint.rules.key-ordering]\n[lint.rules.comments]\n",
    );
    let path = dir.path().join("in.yaml");
    for header in [
        "# $schema: none",
        "# yaml-language-server:",
        "#yaml-language-server: $schema=deny.json",
        "#$schema: deny.json",
    ] {
        let source = format!("{header}\nz: 1\na: 2\n");
        for rewrite in [ryl::fix::Rewrite::Fix, ryl::fix::Rewrite::Format] {
            let (output, notices) = ryl::fix::rewrite_str(
                &source,
                &ctx.config,
                &path,
                &ctx.base_dir,
                SourceKind::Yaml,
                rewrite,
            );
            let output = output.as_deref().unwrap_or(&source);
            assert!(output.contains(header), "{output}");
            if header.starts_with("# ") {
                assert!(output.find(header).unwrap() < output.find("z:").unwrap());
                assert!(
                    rewrite == ryl::fix::Rewrite::Format
                        || notices
                            .iter()
                            .any(|notice| notice.rule == Some("key-ordering"))
                );
            }
            let again = ryl::fix::rewrite_str(
                output,
                &ctx.config,
                &path,
                &ctx.base_dir,
                SourceKind::Yaml,
                rewrite,
            )
            .0;
            assert!(again.is_none() || again.as_deref() == Some(output));
        }
    }
}
