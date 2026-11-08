use std::fs;
use std::path::Path;

use ryl::config::{Overrides, discover_config};
use ryl::lint::lint_str;
use serde_json::{Value, json};

use crate::common::cli::{run, run_with_stdin, ryl};

pub(crate) fn config(dir: &Path, schema: &Value) -> ryl::config::ConfigContext {
    fs::write(dir.join("schema.json"), schema.to_string()).unwrap();
    fs::write(
        dir.join(".ryl.toml"),
        "[lint.rules.schema]\n[[lint.rules.schema.schemas]]\npath = 'schema.json'\n",
    )
    .unwrap();
    discover_config(
        &[],
        &Overrides {
            config_file: Some(dir.join(".ryl.toml")),
            config_data: None,
        },
    )
    .unwrap()
}

pub(crate) fn findings(schema: &Value, yaml: &str) -> Vec<ryl::lint::LintProblem> {
    let dir = tempfile::tempdir().unwrap();
    let ctx = config(dir.path(), schema);
    lint_str(
        yaml,
        &dir.path().join("in.yaml"),
        &ctx.config,
        &ctx.base_dir,
    )
}

#[test]
fn alias_diagnostics_blame_the_use_site() {
    let hits = findings(
        &json!({"properties":{"copy":{"type":"string"}}}),
        "base: &base {x: 1}\ncopy: *base\n",
    );
    assert_eq!((hits[0].line, hits[0].column), (2, 7));
}

#[test]
fn escaped_pointers_and_additional_properties_blame_keys() {
    let hits = findings(
        &json!({"properties":{"a/b~c":{"type":"integer"}},"additionalProperties":false}),
        "a/b~c: nope\nextra: 1\n",
    );
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().any(|hit| (hit.line, hit.column) == (1, 8)));
    assert!(hits.iter().any(|hit| (hit.line, hit.column) == (2, 1)));
}

#[test]
fn yaml_projection_refuses_ambiguous_or_unrepresentable_nodes() {
    for (input, reason) in [
        ("a: 1\n'a': 2\n", "duplicate mapping key"),
        ("1: value\n", "non-string or complex mapping key"),
        ("? [a, b]\n: value\n", "non-string or complex mapping key"),
        ("18446744073709551616\n", "integer outside i64/u64 range"),
        ("-.inf\n", "non-finite float"),
        ("!Foo value\n", "tag !Foo"),
        ("!!int nope\n", "invalid scalar tag"),
        ("!Foo [a]\n", "tag !Foo"),
        ("a: *missing\n", "incomplete YAML document"),
        (
            "%YAML 1.1\n---\n2001-01-01\n",
            "unsupported YAML 1.1 scalar",
        ),
    ] {
        let hits = findings(&json!(true), input);
        assert!(
            hits.iter()
                .any(|hit| hit.message == format!("cannot validate: {reason}")),
            "{input:?}: {hits:?}"
        );
    }
}

#[test]
fn wide_integers_floats_core_tags_and_explicit_yaml11_resolve() {
    for (input, value) in [
        ("9223372036854775808", json!(9223372036854775808_u64)),
        ("18446744073709551615", json!(u64::MAX)),
        ("-9223372036854775808", json!(i64::MIN)),
        ("0xB", json!(11)),
        ("0o13", json!(11)),
        ("011", json!(11)),
        (".5", json!(0.5)),
        ("true", json!(true)),
        ("null", Value::Null),
        ("!!str 1", json!("1")),
        ("!!int '2'", json!(2)),
        ("! 2", json!("2")),
        ("'2'", json!("2")),
        ("%YAML 1.1\n---\nyes", json!(true)),
        ("%YAML 1.1\n---\n011", json!(9)),
        ("%YAML 1.1\n---\n1.5", json!(1.5)),
        ("%YAML 1.1\n---\nhello", json!("hello")),
        ("%YAML 1.1\n---\nnull", Value::Null),
    ] {
        assert!(
            findings(&json!({"const":value}), &format!("{input}\n")).is_empty(),
            "{input}"
        );
    }
}

#[test]
fn merge_expansion_obeys_explicit_and_sequence_precedence() {
    let schema = json!({"properties":{"job":{"type":"object","properties":{"script":{"const":"test"},"image":{"const":"rust"}},"required":["script","image"],"additionalProperties":false}}});
    for yaml in [
        "base: &base {script: test}\njob: {<<: *base, image: rust}\n",
        "job: {script: test, <<: {script: ignored, image: rust}}\n",
        "job: {<<: [{script: test}, {script: ignored, image: rust}]}\n",
        "job: {<<: {script: ignored, image: rust}, script: test}\n",
        "job: {!!merge '<<': {script: test, image: rust}}\n",
    ] {
        assert!(findings(&schema, yaml).is_empty(), "{yaml}");
    }
    for yaml in [
        "job: {<<: bad}\n",
        "job: {<<: [bad]}\n",
        "job: {<<: {}, <<: {}}\n",
    ] {
        assert!(
            findings(&schema, yaml)[0]
                .message
                .contains("cannot validate")
        );
    }
    assert!(findings(&json!({"required":["<<"]}), "'<<': value\n").is_empty());
}

#[test]
fn documents_are_validated_independently_including_empty_documents() {
    assert_eq!(
        findings(&json!({"type":"integer"}), "---\n1\n---\nnope\n---\n").len(),
        2
    );
    assert!(findings(&json!(true), "").is_empty());
}

#[test]
fn projection_bounds_aliases_bytes_and_depth() {
    let mut aliases = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for i in 1..8 {
        aliases.push_str(&format!(
            "a{i}: &a{i} [{}]\n",
            vec![format!("*a{}", i - 1); 10].join(", ")
        ));
    }
    assert!(
        findings(&json!(true), &aliases)[0]
            .message
            .contains("resource limit")
    );
    let large = format!("{}\n", "x".repeat(16 * 1024 * 1024 + 1));
    assert!(
        findings(&json!(true), &large)[0]
            .message
            .contains("resource limit")
    );
    let deep = format!("{}0{}\n", "[".repeat(130), "]".repeat(130));
    assert!(
        findings(&json!(true), &deep)[0]
            .message
            .contains("depth limit")
    );
}

#[test]
fn diagnostics_are_capped_per_file_with_a_truncation_note() {
    let yaml = format!(
        "{}\n---\n{}\n",
        "- nope\n".repeat(100),
        "- nope\n".repeat(2)
    );
    let hits = findings(&json!({"items":{"type":"integer"}}), &yaml);
    assert_eq!(hits.len(), 101);
    assert!(hits.iter().any(|hit| hit.message.contains("truncated")));
}

#[test]
fn first_matching_association_wins_with_negated_globs() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!(false));
    fs::write(dir.path().join("other.json"), "true").unwrap();
    let cfg = "[[lint.rules.schema.schemas]]\npath = 'schema.json'\nfiles = ['*.yaml', '!skip.yaml']\n[[lint.rules.schema.schemas]]\npath = 'other.json'\n";
    fs::write(dir.path().join(".ryl.toml"), cfg).unwrap();
    for (name, expected) in [("in.yaml", 1), ("skip.yaml", 0)] {
        fs::write(dir.path().join(name), "a: 1\n").unwrap();
        let result = run(ryl(dir.path())
            .args(["check", "--no-cache", "-c"])
            .arg(dir.path().join(".ryl.toml"))
            .arg(dir.path().join(name)));
        assert_eq!(result.0, expected, "{result:?}");
    }
}

#[test]
fn stdin_filename_markdown_and_directives_use_normal_rule_routing() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!({"required":["needed"]}));
    let cfg = dir.path().join(".ryl.toml");
    for (yaml, expected) in [
        ("a: 1\n", 1),
        ("# ryl disable rule:schema\na: 1\n", 0),
        ("a: 1 # ryl disable-line rule:schema\n", 0),
    ] {
        let result = run_with_stdin(
            ryl(dir.path()).args(["check", "-c"]).arg(&cfg).args([
                "--stdin-filename",
                "in.yaml",
                "-",
            ]),
            yaml.as_bytes(),
        );
        assert_eq!(result.0, expected, "{result:?}");
    }
    fs::write(dir.path().join("in.md"), "---\na: 1\n---\nbody\n").unwrap();
    let result = run(ryl(dir.path())
        .args(["check", "--markdown", "-c"])
        .arg(&cfg)
        .arg(dir.path().join("in.md")));
    assert_eq!(result.0, 1, "{result:?}");
    assert!(
        format!("{}{}", result.1, result.2).contains("2:1"),
        "{result:?}"
    );
    fs::write(
        &cfg,
        "[[lint.rules.schema.schemas]]\npath = 'schema.json'\nfiles = ['*.yaml']\n",
    )
    .unwrap();
    assert_eq!(
        run_with_stdin(
            ryl(dir.path()).args(["check", "-c"]).arg(&cfg).arg("-"),
            b"a: 1\n"
        )
        .0,
        0
    );
}

#[test]
fn all_without_associations_and_schema_free_debug_remain_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let env =
        crate::common::fake_env::FakeEnv::new().with_cwd(dir.path().join("missing"));
    let cfg = ryl::config::discover_config_with(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("[lint.rules]\nALL = 'enable'\n".to_owned()),
        },
        &env,
    )
    .unwrap()
    .config;
    assert!(format!("{cfg:?}").ends_with("resolved: Resolved }"));
    assert!(ryl::rules::schema::check("a: 1\n", &cfg, Path::new("a.yaml")).is_empty());
}

fn failure(dir: &Path, expected: &str) {
    fs::write(dir.join("in.yaml"), "a: 1\n").unwrap();
    let result = run(ryl(dir)
        .args(["check", "-c"])
        .arg(dir.join(".ryl.toml"))
        .arg(dir.join("in.yaml")));
    assert_eq!(result.0, 2, "{result:?}");
    assert!(result.2.contains(expected), "{result:?}");
}

#[test]
fn schema_config_errors_fail_before_linting() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("outside.json"), "true").unwrap();
    for (schema, expected) in [
        ("{", "schema.json"),
        (r#"{"type":"invalid"}"#, "schema.json"),
        (
            r#"{"$ref":"https://example.invalid/schema"}"#,
            "network opt-in",
        ),
        (
            r#"{"$defs":{"unused":{"$ref":"https://example.invalid/schema"}}}"#,
            "network opt-in",
        ),
        (r#"{"$ref":"../outside.json"}"#, "escapes config"),
        (
            r#"{"$ref":"file://user@localhost/schema.json"}"#,
            "invalid international domain name",
        ),
    ] {
        config(dir.path(), &json!(true));
        let outside_uri =
            url::Url::from_file_path(outside.path().join("outside.json")).unwrap();
        fs::write(
            dir.path().join("schema.json"),
            schema.replace("../outside.json", outside_uri.as_str()),
        )
        .unwrap();
        failure(dir.path(), expected);
    }
    fs::remove_file(dir.path().join("schema.json")).unwrap();
    failure(dir.path(), "schema.json");
    fs::write(
        dir.path().join("schema.json"),
        format!("{}true", " ".repeat(16 * 1024 * 1024)),
    )
    .unwrap();
    failure(dir.path(), "size limit");
}

#[test]
fn configured_root_is_confined_like_references() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("outside.json"), "true").unwrap();
    for path in [
        outside
            .path()
            .join("outside.json")
            .to_string_lossy()
            .into_owned(),
        format!(
            "../{}/outside.json",
            outside.path().file_name().unwrap().to_str().unwrap()
        ),
    ] {
        fs::write(
            dir.path().join(".ryl.toml"),
            format!("[[lint.rules.schema.schemas]]\npath = '{path}'\n"),
        )
        .unwrap();
        failure(dir.path(), "schema");
    }
}

#[cfg(unix)]
#[test]
fn root_and_ref_symlinks_cannot_escape_the_config_directory() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("outside.json"), "true").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("outside.json"),
        dir.path().join("escape.json"),
    )
    .unwrap();
    config(dir.path(), &json!(true));
    fs::write(dir.path().join("schema.json"), r#"{"$ref":"escape.json"}"#).unwrap();
    failure(dir.path(), "escapes config");
    fs::write(
        dir.path().join(".ryl.toml"),
        "[[lint.rules.schema.schemas]]\npath = 'escape.json'\n",
    )
    .unwrap();
    failure(dir.path(), "escapes config");
}

#[test]
fn referenced_schema_edits_invalidate_cache_after_an_unmatched_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("child.json"), "true").unwrap();
    config(dir.path(), &json!({"$ref":"child.json"}));
    fs::write(dir.path().join(".ryl.toml"), "[[lint.rules.schema.schemas]]\npath = 'schema.json'\nfiles = ['matched.yaml']\n").unwrap();
    fs::write(dir.path().join("child.json"), "true").unwrap();
    for name in ["unmatched.yaml", "matched.yaml"] {
        fs::write(dir.path().join(name), "a: 1\n").unwrap();
    }
    let check = || {
        run(ryl(dir.path())
            .env_remove("RYL_NO_CACHE")
            .args(["check", "--cache-dir"])
            .arg(dir.path().join("cache"))
            .arg("-c")
            .arg(dir.path().join(".ryl.toml"))
            .arg(dir.path().join("unmatched.yaml"))
            .arg(dir.path().join("matched.yaml")))
    };
    assert_eq!(check().0, 0);
    assert!(
        fs::read_dir(dir.path().join("cache").join(env!("CARGO_PKG_VERSION")))
            .unwrap()
            .next()
            .is_some()
    );
    assert_eq!(check().0, 0);
    fs::write(dir.path().join("child.json"), "false").unwrap();
    assert_eq!(check().0, 1);
}

#[test]
fn vendored_real_schemas_accept_representative_documents() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "github-workflow",
        "compose-spec",
        "pre-commit-config",
        "pre-commit-hooks",
    ] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/schema/{name}.json")),
            dir.path().join(format!("{name}.json")),
        )
        .unwrap();
    }
    for (schema, yaml) in [
        (
            "github-workflow",
            "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo ok\n",
        ),
        ("compose-spec", "services:\n  web:\n    image: nginx\n"),
        (
            "pre-commit-config",
            "repos:\n  - repo: local\n    hooks:\n      - id: test\n        name: test\n        entry: echo\n        language: system\n",
        ),
    ] {
        fs::write(
            dir.path().join(".ryl.toml"),
            format!("[[lint.rules.schema.schemas]]\npath = '{schema}.json'\n"),
        )
        .unwrap();
        let ctx = discover_config(
            &[],
            &Overrides {
                config_file: Some(dir.path().join(".ryl.toml")),
                config_data: None,
            },
        )
        .unwrap();
        let hits = lint_str(
            yaml,
            &dir.path().join("in.yaml"),
            &ctx.config,
            &ctx.base_dir,
        );
        assert!(hits.is_empty(), "{schema}: {hits:?}");
    }
}

#[test]
fn format_does_not_load_missing_lint_schemas() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[[lint.rules.schema.schemas]]\npath = 'missing.json'\n",
    )
    .unwrap();
    fs::write(dir.path().join("in.yaml"), "a: 1\n").unwrap();
    for explicit in [false, true] {
        let mut command = ryl(dir.path());
        command.arg("format");
        if explicit {
            command.arg("-c").arg(dir.path().join(".ryl.toml"));
        }
        let result = run(command.arg(dir.path().join("in.yaml")));
        assert_eq!(result.0, 0, "{result:?}");
    }
    let result = run_with_stdin(
        ryl(dir.path())
            .args(["format", "-c"])
            .arg(dir.path().join(".ryl.toml"))
            .arg("-"),
        b"a: 1\n",
    );
    assert_eq!(result.0, 0, "{result:?}");
}

#[cfg(feature = "lsp")]
#[test]
fn schema_editor_diagnostics_count_utf16_after_astral_characters() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = config(dir.path(), &json!({"properties":{"😀":{"type":"integer"}}}));
    let hits = ryl::lsp::analysis::diagnostics(
        "{😀: nope}\n",
        &dir.path().join("in.yaml"),
        &ctx.config,
        &ctx.base_dir,
        ryl::config::SourceKind::Yaml,
        ryl::lsp::encoding::PositionEncoding::Utf16,
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].range.start.character, 5);
}

#[test]
fn bounded_regex_failure_is_a_cannot_validate_diagnostic() {
    let input = format!("'{}!'\n", "a".repeat(80));
    let hits = findings(&json!({"pattern":r"^(a|aa)+\1$"}), &input);
    assert!(
        hits.iter()
            .any(|hit| hit.message.starts_with("cannot validate:")),
        "{hits:?}"
    );
}

#[test]
fn cyclic_aliases_and_expanded_depth_are_refused() {
    assert!(
        findings(&json!(true), "a: &a [*a]\n")[0]
            .message
            .contains("unresolved alias")
    );
    let yaml = format!(
        "a: &a {}x{}\nb: {}*a{}\n",
        "[".repeat(70),
        "]".repeat(70),
        "[".repeat(70),
        "]".repeat(70)
    );
    assert!(
        findings(&json!(true), &yaml)[0]
            .message
            .contains("depth limit")
    );
}

#[test]
fn schema_ref_limit_and_combined_byte_limit_are_config_errors() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!(true));
    let refs = (0..1024)
        .map(|i| {
            let name = format!("child{i}.json");
            fs::write(dir.path().join(&name), "true").unwrap();
            json!({"$ref": name})
        })
        .collect::<Vec<_>>();
    fs::write(
        dir.path().join("schema.json"),
        json!({"allOf":refs}).to_string(),
    )
    .unwrap();
    failure(dir.path(), "resource limit");
    fs::write(
        dir.path().join("schema.json"),
        json!({"$ref":"large.json"}).to_string(),
    )
    .unwrap();
    fs::write(
        dir.path().join("large.json"),
        format!("{}true", " ".repeat(16 * 1024 * 1024)),
    )
    .unwrap();
    failure(dir.path(), "size limit");
}

#[test]
fn local_uri_conversion_decodes_escaped_filenames_and_refuses_unc() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("child space.json"), "true").unwrap();
    let ctx = config(dir.path(), &json!({"$ref":"child%20space.json"}));
    assert!(
        lint_str(
            "1\n",
            &dir.path().join("in.yaml"),
            &ctx.config,
            &ctx.base_dir
        )
        .is_empty()
    );
    fs::write(
        dir.path().join("schema.json"),
        json!({"$ref":"file://other-host/share/schema.json"}).to_string(),
    )
    .unwrap();
    failure(dir.path(), "UNC");
}

#[test]
fn schema_config_validates_globs_and_required_paths() {
    for config in [
        "[[lint.rules.schema.schemas]]\npath = 'schema.json'\nfiles = ['[']\n",
        "[[lint.rules.schema.schemas]]\nfiles = []\n",
        "[lint.rules.schema]\nschemas = 1\n",
    ] {
        assert!(ryl::config::YamlLintConfig::from_toml_str(config).is_err());
    }
}

#[test]
fn nested_merges_preserve_values_and_replace_descendant_spans() {
    let expected = json!({"job":{"a":0,"z":{"array":[true,null,1,"text"]}}});
    let yaml =
        "job: {<<: {a: {nested: wrong}, z: {array: [true, null, 1, text]}}, a: 0}\n";
    assert!(findings(&json!({"const":expected}), yaml).is_empty());
}

#[test]
fn regex_failures_inside_combinators_and_property_names_are_refused() {
    let pattern = json!({"pattern":r"^(a|aa)+\1$"});
    let value = format!("{}!", "a".repeat(80));
    for schema in [
        json!({"anyOf":[pattern,{"type":"integer"}]}),
        json!({"oneOf":[pattern,{"type":"integer"}]}),
    ] {
        let hits = findings(&schema, &format!("'{value}'\n"));
        assert!(
            hits[0].message.starts_with("cannot validate:"),
            "{schema}: {hits:?}"
        );
    }
    assert!(
        findings(
            &json!({"propertyNames":pattern}),
            &format!("'{value}': 1\n")
        )[0]
        .message
        .starts_with("cannot validate:")
    );
}

#[test]
fn explicit_yaml11_core_types_and_inherited_scalar_anchors_resolve() {
    for (yaml, expected) in [
        ("!!int '11'", json!(11)),
        ("!!bool yes", json!(true)),
        ("!!float '1.5'", json!(1.5)),
    ] {
        assert!(
            findings(
                &json!({"const":expected}),
                &format!("%YAML 1.1\n---\n{yaml}\n")
            )
            .is_empty()
        );
    }
    assert!(findings(&json!({"const":{"a":1,"b":1}}), "a: &a 1\nb: *a\n").is_empty());
}

#[test]
fn projection_budget_bounds_each_kind_of_allocation() {
    let limit = 16 * 1024 * 1024;
    for (allocation, yaml) in [
        (
            "anchor storage",
            format!("a: &a '{}'\n", "x".repeat(9 * 1024 * 1024)),
        ),
        (
            "alias expansion",
            format!("a: &a '{}'\nb: *a\n", "x".repeat(6 * 1024 * 1024)),
        ),
        (
            "mapping value pointer",
            format!("a: {}\n", "x".repeat(limit - 1)),
        ),
        (
            "nested key pointer",
            format!("a: {{b: {}}}\n", "x".repeat(limit - 12)),
        ),
        (
            "merge subtree",
            format!("job: {{<<: {{x: {}}}}}\n", "x".repeat(9 * 1024 * 1024)),
        ),
        (
            "merge value pointer",
            format!("job: {{<<: {{x: {}}}}}\n", "x".repeat((limit - 8) / 2)),
        ),
        ("node count", format!("[{}~]\n", "~,".repeat(1_000_000))),
    ] {
        let hits = findings(&json!(true), &yaml);
        assert!(
            hits[0].message.contains("resource limit"),
            "{allocation}: {:?}",
            hits[0]
        );
    }
}

#[test]
fn a_projection_refusal_after_the_diagnostic_cap_reports_truncation() {
    let yaml = format!("{}---\n!Foo value\n", "- nope\n".repeat(100));
    let hits = findings(&json!({"items":{"type":"integer"}}), &yaml);
    assert_eq!(hits.len(), 101);
    assert!(hits.iter().any(|hit| hit.message.contains("truncated")));
}

#[test]
fn repeated_schema_paths_and_unreadable_roots_use_the_confined_reader() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!(true));
    fs::write(dir.path().join(".ryl.toml"), "[[lint.rules.schema.schemas]]\npath = 'schema.json'\nfiles = ['skip.yaml']\n[[lint.rules.schema.schemas]]\npath = 'schema.json'\n").unwrap();
    fs::write(dir.path().join("in.yaml"), "1\n").unwrap();
    assert_eq!(
        run(ryl(dir.path())
            .args(["check", "-c"])
            .arg(dir.path().join(".ryl.toml"))
            .arg(dir.path().join("in.yaml")))
        .0,
        0
    );
    fs::write(
        dir.path().join(".ryl.toml"),
        "[[lint.rules.schema.schemas]]\npath = '.'\n",
    )
    .unwrap();
    failure(dir.path(), "schema");
}

#[test]
fn schema_config_requires_an_existing_base_directory() {
    let dir = tempfile::tempdir().unwrap();
    let env =
        crate::common::fake_env::FakeEnv::new().with_cwd(dir.path().join("missing"));
    let result = ryl::config::discover_config_with(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(
                "[[lint.rules.schema.schemas]]\npath = 'schema.json'\n".to_owned(),
            ),
        },
        &env,
    );
    assert!(result.unwrap_err().contains("schema base"));
}

#[test]
fn schema_selectors_work_in_per_file_and_per_line_ignores() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!(false));
    for ignores in [
        "[lint.per-file-ignores]\n'*.yaml' = ['schema']\n",
        "[[lint.per-line-ignores]]\nregex = '^skip$'\nrules = ['schema']\n",
    ] {
        let text =
            format!("{ignores}[[lint.rules.schema.schemas]]\npath = 'schema.json'\n");
        fs::write(dir.path().join(".ryl.toml"), text).unwrap();
        let ctx = discover_config(
            &[],
            &Overrides {
                config_file: Some(dir.path().join(".ryl.toml")),
                config_data: None,
            },
        )
        .unwrap();
        assert!(
            lint_str(
                "skip\n",
                &dir.path().join("in.yaml"),
                &ctx.config,
                &ctx.base_dir
            )
            .is_empty()
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_drive_relative_roots_are_not_config_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[[lint.rules.schema.schemas]]\npath = 'C:outside.json'\n",
    )
    .unwrap();
    failure(dir.path(), "root path must be relative");
}

#[test]
fn markdown_caps_schema_findings_across_regions_and_keeps_other_rules() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!({"items":{"type":"integer"}}));
    fs::write(dir.path().join(".ryl.toml"), "[lint.rules]\ntrailing-spaces = 'enable'\n[[lint.rules.schema.schemas]]\npath = 'schema.json'\n").unwrap();
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(dir.path().join(".ryl.toml")),
            config_data: None,
        },
    )
    .unwrap();
    for size in [60, 120] {
        let region = format!("```yaml\n{}```\n", "- bad  \n".repeat(size));
        let hits = ryl::markdown_embed::lint_markdown_str(
            &region.repeat(2),
            &dir.path().join("in.md"),
            &ctx.config,
            &ctx.base_dir,
        );
        assert_eq!(
            hits.iter().filter(|hit| hit.rule == Some("schema")).count(),
            101
        );
        assert_eq!(
            hits.iter()
                .filter(|hit| hit.message.contains("truncated"))
                .count(),
            1
        );
        assert_eq!(
            hits.iter()
                .filter(|hit| hit.rule == Some("trailing-spaces"))
                .count(),
            size * 2
        );
    }
}

#[test]
fn markdown_without_a_matching_schema_keeps_all_other_findings() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!(false));
    fs::write(dir.path().join(".ryl.toml"), "[lint.rules]\ntrailing-spaces = 'enable'\n[[lint.rules.schema.schemas]]\npath = 'schema.json'\nfiles = ['*.yaml']\n").unwrap();
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(dir.path().join(".ryl.toml")),
            config_data: None,
        },
    )
    .unwrap();
    let markdown = format!("```yaml\n{}```\n", "- bad  \n".repeat(120));
    let hits = ryl::markdown_embed::lint_markdown_str(
        &markdown,
        &dir.path().join("in.md"),
        &ctx.config,
        &ctx.base_dir,
    );
    assert_eq!(hits.len(), 120);
    assert!(hits.iter().all(|hit| hit.rule == Some("trailing-spaces")));
}

#[test]
fn schema_size_budgets_are_independent_for_separate_roots() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["first.json", "second.json"] {
        fs::write(
            dir.path().join(name),
            json!({"$comment":"x".repeat(9 * 1024 * 1024),"type":"integer"})
                .to_string(),
        )
        .unwrap();
    }
    fs::write(dir.path().join(".ryl.toml"), "[[lint.rules.schema.schemas]]\npath = 'first.json'\n[[lint.rules.schema.schemas]]\npath = 'second.json'\n").unwrap();
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: Some(dir.path().join(".ryl.toml")),
            config_data: None,
        },
    )
    .unwrap();
    assert!(
        lint_str(
            "1\n",
            &dir.path().join("in.yaml"),
            &ctx.config,
            &ctx.base_dir
        )
        .is_empty()
    );
}

#[test]
fn local_ref_query_aliases_reuse_the_confined_file_contents() {
    assert!(findings(&json!({"$defs":{"value":{"type":"integer"}},"$ref":"schema.json?alias#/$defs/value"}), "1\n").is_empty());
}

#[test]
fn distinct_uri_aliases_count_toward_resource_and_byte_limits() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!(true));
    fs::write(dir.path().join("child.json"), "true").unwrap();
    let refs = (0..1024)
        .map(|i| json!({"$ref":format!("child.json?alias{i}")}))
        .collect::<Vec<_>>();
    fs::write(
        dir.path().join("schema.json"),
        json!({"allOf":refs}).to_string(),
    )
    .unwrap();
    failure(dir.path(), "resource limit");
    fs::write(
        dir.path().join("schema.json"),
        json!({"$comment":"x".repeat(8 * 1024 * 1024),"$defs":{"value":true},"$ref":"schema.json?alias#/$defs/value"}).to_string(),
    ).unwrap();
    failure(dir.path(), "size limit");
}

#[test]
fn unsupported_documents_do_not_skip_later_documents() {
    let hits = findings(
        &json!({"type":"object","required":["needed"]}),
        "---\n!Foo value\n---\nother: 1\n---\nneeded: 1\n",
    );
    assert_eq!(hits.len(), 2);
    assert!(
        hits.iter()
            .any(|hit| hit.line == 2 && hit.message.contains("tag !Foo"))
    );
    assert!(
        hits.iter()
            .any(|hit| hit.line == 4 && hit.message.contains("required property"))
    );
}

#[test]
fn a_bare_relative_config_path_anchors_schemas_at_the_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    config(dir.path(), &json!(false));
    fs::write(dir.path().join("in.yaml"), "1\n").unwrap();
    let result = run(ryl(dir.path()).current_dir(dir.path()).args([
        "check",
        "--no-cache",
        "-c",
        ".ryl.toml",
        "in.yaml",
    ]));
    assert_eq!(result.0, 1, "{result:?}");
    assert!(format!("{}{}", result.1, result.2).contains("schema"));
}
