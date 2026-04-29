use std::{fs, process::Command};

use jsonschema::validator_for;
use ryl::config_schema::{
    NormalizedConfig, normalize_toml_config, normalized_config_to_toml_value,
    parse_toml_config_str, schema_value, toml_config_to_value, validate_toml_config,
    yaml_schema_value,
};
use serde_json::{Value, json};

fn toml_to_json(input: &str) -> Value {
    let parsed: toml::Table = input.parse().expect("valid TOML fixture");
    serde_json::to_value(parsed).expect("TOML value should serialize to JSON")
}

fn properties_for_ref<'a>(schema: &'a Value, property: &str) -> &'a Value {
    let target = schema
        .get("properties")
        .and_then(|root| root.get(property))
        .expect("schema property should exist");

    let reference = target
        .get("$ref")
        .and_then(Value::as_str)
        .or_else(|| {
            target
                .get("anyOf")
                .and_then(Value::as_array)
                .and_then(|variants| {
                    variants
                        .iter()
                        .find_map(|variant| variant.get("$ref").and_then(Value::as_str))
                })
        })
        .expect("schema property should reference a definition");
    let (definitions_key, definition) = reference
        .strip_prefix("#/$defs/")
        .map(|definition| ("$defs", definition))
        .or_else(|| {
            reference
                .strip_prefix("#/definitions/")
                .map(|definition| ("definitions", definition))
        })
        .expect("schema ref should point into a definitions map");

    schema
        .get(definitions_key)
        .and_then(|defs| defs.get(definition))
        .and_then(|entry| entry.get("properties"))
        .expect("schema definition properties should exist")
}

fn checked_in_schema(path: &str) -> Value {
    let root = env!("CARGO_MANIFEST_DIR");
    let data = fs::read_to_string(format!("{root}/{path}"))
        .expect("checked-in schema artifact should exist");
    serde_json::from_str(&data)
        .expect("checked-in schema artifact should be valid JSON")
}

fn checked_in_text(path: &str) -> String {
    let root = env!("CARGO_MANIFEST_DIR");
    fs::read_to_string(format!("{root}/{path}"))
        .expect("checked-in artifact should exist")
}

fn schemastore_yamllint_schema() -> Value {
    checked_in_schema("tests/fixtures/schemastore-yamllint.json")
}

struct SchemaComparisonCase {
    name: &'static str,
    instance: Value,
    expected: bool,
}

fn sampled_yaml_schema_comparison_cases() -> Vec<SchemaComparisonCase> {
    vec![
        SchemaComparisonCase {
            name: "extends-string",
            instance: json!({ "extends": "default" }),
            expected: true,
        },
        SchemaComparisonCase {
            name: "extends-sequence",
            instance: json!({ "extends": ["default", "relaxed"] }),
            expected: false,
        },
        SchemaComparisonCase {
            name: "yaml-files-list",
            instance: json!({ "yaml-files": ["*.yaml", "*.yml"] }),
            expected: true,
        },
        SchemaComparisonCase {
            name: "yaml-files-scalar",
            instance: json!({ "yaml-files": "*.yaml" }),
            expected: false,
        },
        SchemaComparisonCase {
            name: "ignore-string",
            instance: json!({ "ignore": "vendor/**\ngenerated/**" }),
            expected: true,
        },
        SchemaComparisonCase {
            name: "ignore-from-file-list",
            instance: json!({ "ignore-from-file": [".gitignore", ".yamlignore"] }),
            expected: true,
        },
        SchemaComparisonCase {
            name: "ignore-mutually-exclusive",
            instance: json!({
                "ignore": "vendor/**",
                "ignore-from-file": ".gitignore"
            }),
            expected: false,
        },
        SchemaComparisonCase {
            name: "rule-mapping",
            instance: json!({
                "rules": {
                    "line-length": {
                        "max": 80
                    }
                }
            }),
            expected: true,
        },
    ]
}

fn assert_readable_rule_wrapper_defs(schema: &Value) {
    let defs = schema
        .get("$defs")
        .or_else(|| schema.get("definitions"))
        .and_then(Value::as_object)
        .expect("schema defs should exist");

    assert!(defs.contains_key("RuleEntryForAnchorsOptions"));
    assert!(defs.contains_key("RuleOptionsForAnchorsOptions"));
    assert!(defs.keys().all(|key| {
        !has_numbered_rule_wrapper_suffix(key, "RuleEntry")
            && !has_numbered_rule_wrapper_suffix(key, "RuleOptions")
    }));
}

fn has_numbered_rule_wrapper_suffix(key: &str, prefix: &str) -> bool {
    key.strip_prefix(prefix).is_some_and(|suffix| {
        !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
    })
}

fn printed_schema(flag: &str) -> Value {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let out = Command::new(exe)
        .arg(flag)
        .output()
        .expect("schema command should run");

    assert!(
        out.status.success(),
        "schema command should succeed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());

    let stdout =
        String::from_utf8(out.stdout).expect("schema output should be valid UTF-8");
    serde_json::from_str(&stdout).expect("schema output should be JSON")
}

fn printed_schemastore_toml_schema() -> Value {
    let root = env!("CARGO_MANIFEST_DIR");
    let out = Command::new("uv")
        .arg("run")
        .arg("scripts/print_ryl_schemastore_schema.py")
        .current_dir(root)
        .output()
        .expect("SchemaStore schema script should run");

    assert!(
        out.status.success(),
        "SchemaStore schema script should succeed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());

    let stdout =
        String::from_utf8(out.stdout).expect("schema output should be valid UTF-8");
    serde_json::from_str(&stdout).expect("schema output should be JSON")
}

fn schema_contains_int64_format(node: &Value) -> bool {
    match node {
        Value::Object(map) => map.iter().any(|(key, value)| {
            (key == "format" && value == "int64") || schema_contains_int64_format(value)
        }),
        Value::Array(values) => values.iter().any(schema_contains_int64_format),
        _ => false,
    }
}

#[test]
fn generated_schema_accepts_valid_sample_config() {
    let schema = schema_value();
    let validator = validator_for(&schema).expect("generated schema should compile");
    let instance = toml_to_json(
        r#"
yaml-files = ["*.yaml", "*.yml"]
ignore = ["vendor/**", "generated/**"]
locale = "en_US.UTF-8"

[per-file-ignores]
"**/values.yaml" = ["document-start"]

[rules]
document-start = "disable"
comments-indentation = true
new-line-at-end-of-file = "enable"

[rules.comments]
level = "warning"
require-starting-space = true
ignore = ["generated.yaml"]

[rules.indentation]
spaces = "consistent"
indent-sequences = "whatever"
check-multi-line-strings = false

[rules.quoted-strings]
quote-type = "double"
required = "only-when-needed"
extra-required = ["^cmd$"]
allow-double-quotes-for-escaping = true
check-keys = true

[fix]
fixable = ["ALL"]
unfixable = ["comments"]
"#,
    );

    assert!(
        validator.is_valid(&instance),
        "schema should accept sample config"
    );
}

#[test]
fn generated_schema_rejects_invalid_known_field_types() {
    let schema = schema_value();
    let validator = validator_for(&schema).expect("generated schema should compile");
    let instance = toml_to_json(
        r#"
[rules.comments]
require-starting-space = "yes"

[fix]
fixable = "comments"

[per-file-ignores]
"values.yaml" = ["not-a-rule"]
"#,
    );

    assert!(
        !validator.is_valid(&instance),
        "schema should reject invalid field types"
    );
}

#[test]
fn normalize_toml_config_preserves_all_per_file_ignore_rule_names() {
    let typed = parse_toml_config_str(
        r#"
[per-file-ignores]
"all.yaml" = [
    "anchors",
    "braces",
    "brackets",
    "colons",
    "commas",
    "comments",
    "comments-indentation",
    "document-end",
    "document-start",
    "empty-lines",
    "empty-values",
    "float-values",
    "hyphens",
    "indentation",
    "key-duplicates",
    "key-ordering",
    "line-length",
    "new-line-at-end-of-file",
    "new-lines",
    "octal-values",
    "quoted-strings",
    "trailing-spaces",
    "truthy",
]
"#,
        false,
    )
    .unwrap()
    .unwrap();
    let normalized = normalize_toml_config(&typed);
    let rules = normalized
        .per_file_ignores
        .get("all.yaml")
        .expect("per-file rule list should normalize");
    assert_eq!(
        rules,
        &[
            "anchors",
            "braces",
            "brackets",
            "colons",
            "commas",
            "comments",
            "comments-indentation",
            "document-end",
            "document-start",
            "empty-lines",
            "empty-values",
            "float-values",
            "hyphens",
            "indentation",
            "key-duplicates",
            "key-ordering",
            "line-length",
            "new-line-at-end-of-file",
            "new-lines",
            "octal-values",
            "quoted-strings",
            "trailing-spaces",
            "truthy",
        ]
    );
}

#[test]
fn generated_schema_exposes_known_rule_properties() {
    let schema = schema_value();
    let rule_properties = properties_for_ref(&schema, "rules");

    assert!(rule_properties.get("quoted-strings").is_some());
    assert!(rule_properties.get("new-line-at-end-of-file").is_some());
    assert!(rule_properties.get("comments-indentation").is_some());
}

#[test]
fn generated_schema_uses_readable_rule_wrapper_names() {
    assert_readable_rule_wrapper_defs(&schema_value());
}

#[test]
fn cli_print_toml_config_schema_outputs_generated_schema_without_inputs() {
    assert_eq!(printed_schema("--print-toml-config-schema"), schema_value());
}

#[test]
fn schemastore_toml_schema_sets_expected_metadata() {
    let source_schema = schema_value();
    let schemastore_schema = printed_schemastore_toml_schema();

    assert_eq!(
        schemastore_schema.get("$schema").and_then(Value::as_str),
        Some("http://json-schema.org/draft-07/schema#")
    );
    assert_eq!(
        schemastore_schema.get("$id").and_then(Value::as_str),
        Some("https://json.schemastore.org/ryl.json")
    );
    assert!(schemastore_schema.get("$defs").is_none());
    assert_eq!(
        schemastore_schema
            .get("definitions")
            .and_then(Value::as_object)
            .map(|definitions| definitions.len()),
        source_schema
            .get("$defs")
            .and_then(Value::as_object)
            .map(|definitions| definitions.len())
    );
    assert!(!schema_contains_int64_format(&schemastore_schema));
}

#[test]
fn schemastore_toml_schema_exposes_known_rule_properties() {
    let schema = printed_schemastore_toml_schema();
    let rule_properties = properties_for_ref(&schema, "rules");

    assert!(rule_properties.get("quoted-strings").is_some());
    assert!(rule_properties.get("new-line-at-end-of-file").is_some());
    assert!(rule_properties.get("comments-indentation").is_some());
}

#[test]
fn schemastore_toml_schema_uses_readable_rule_wrapper_names() {
    let schema = printed_schemastore_toml_schema();
    assert_readable_rule_wrapper_defs(&schema);
}

#[test]
fn schemastore_toml_schema_validates_repo_fixtures() {
    let schema = printed_schemastore_toml_schema();
    let validator =
        validator_for(&schema).expect("SchemaStore TOML schema should compile");
    let valid = toml_to_json(&checked_in_text(
        "tests/fixtures/schemastore/ryl-valid.toml",
    ));
    let invalid = toml_to_json(&checked_in_text(
        "tests/fixtures/schemastore/ryl-invalid.toml",
    ));

    assert!(validator.is_valid(&valid));
    assert!(!validator.is_valid(&invalid));
}

#[test]
fn generated_yaml_schema_accepts_valid_sample_config() {
    let schema = yaml_schema_value();
    let validator = validator_for(&schema).expect("generated schema should compile");
    let instance = json!({
        "extends": "default",
        "yaml-files": ["*.yaml"],
        "ignore": "vendor/**\ngenerated/**",
        "locale": "en_US.UTF-8",
        "rules": {
            "document-start": "disable",
            "comments": {
                "level": "warning",
                "require-starting-space": true,
                "ignore": ["generated.yaml"]
            },
            "quoted-strings": {
                "required": "only-when-needed",
                "extra-required": ["^cmd$"]
            }
        }
    });

    assert!(
        validator.is_valid(&instance),
        "YAML schema should accept sample config"
    );
}

#[test]
fn generated_yaml_schema_rejects_invalid_known_field_types() {
    let schema = yaml_schema_value();
    let validator = validator_for(&schema).expect("generated schema should compile");
    let instance = json!({
        "yaml-files": "*.yaml",
        "ignore": "vendor/**",
        "ignore-from-file": ".gitignore",
        "rules": {
            "comments": {
                "require-starting-space": "yes"
            }
        }
    });

    assert!(
        !validator.is_valid(&instance),
        "YAML schema should reject invalid field types"
    );
}

#[test]
fn generated_yaml_schema_rejects_toml_only_quoted_strings_option() {
    let schema = yaml_schema_value();
    let validator = validator_for(&schema).expect("generated schema should compile");
    let instance = json!({
        "rules": {
            "quoted-strings": {
                "allow-double-quotes-for-escaping": true
            }
        }
    });

    assert!(
        !validator.is_valid(&instance),
        "YAML schema should reject ryl-only quoted-strings options"
    );
}

#[test]
fn generated_yaml_schema_uses_readable_rule_wrapper_names() {
    assert_readable_rule_wrapper_defs(&yaml_schema_value());
}

#[test]
fn generated_yaml_schema_matches_schemastore_snapshot_for_sampled_configs() {
    let local_schema = yaml_schema_value();
    let local_validator =
        validator_for(&local_schema).expect("generated YAML schema should compile");
    let schemastore_schema = schemastore_yamllint_schema();
    let schemastore_validator = validator_for(&schemastore_schema)
        .expect("SchemaStore yamllint snapshot should compile");
    // This is a sampled verdict comparison, not a full semantic-equivalence check,
    // because the two schemas use different but plausibly equivalent structures.
    for SchemaComparisonCase {
        name,
        instance,
        expected,
    } in sampled_yaml_schema_comparison_cases()
    {
        assert_eq!(
            local_validator.is_valid(&instance),
            expected,
            "generated YAML schema drifted on sampled case {name}"
        );
        assert_eq!(
            schemastore_validator.is_valid(&instance),
            expected,
            "SchemaStore yamllint snapshot drifted on sampled case {name}"
        );
    }
}

#[test]
fn cli_print_yaml_config_schema_outputs_generated_schema_without_inputs() {
    assert_eq!(
        printed_schema("--print-yaml-config-schema"),
        yaml_schema_value()
    );
}

#[test]
fn checked_in_toml_schema_matches_generated_schema() {
    assert_eq!(checked_in_schema("ryl.toml.schema.json"), schema_value());
}

#[test]
fn checked_in_yaml_schema_matches_generated_schema() {
    assert_eq!(
        checked_in_schema("ryl.yaml.schema.json"),
        yaml_schema_value()
    );
}

#[test]
fn checked_in_toml_example_validates_against_schema() {
    let schema = checked_in_schema("ryl.toml.schema.json");
    let validator = validator_for(&schema).expect("checked-in schema should compile");
    let instance = toml_to_json(&checked_in_text(".ryl.toml.example"));

    assert!(
        validator.is_valid(&instance),
        "checked-in TOML example should validate against schema"
    );
}

#[test]
fn checked_in_toml_example_covers_all_builtin_rules() {
    let schema = schema_value();
    let rule_properties = properties_for_ref(&schema, "rules")
        .as_object()
        .expect("schema rule properties should be an object");
    let instance = toml_to_json(&checked_in_text(".ryl.toml.example"));
    let configured_rules = instance
        .get("rules")
        .and_then(Value::as_object)
        .expect("example config should contain rules");

    assert_eq!(configured_rules.len(), rule_properties.len());
    for rule_name in rule_properties.keys() {
        assert!(
            configured_rules.contains_key(rule_name),
            "example config should include rule {rule_name}"
        );
    }
}

#[test]
fn typed_toml_parser_reads_project_toml() {
    let parsed = parse_toml_config_str(
        r#"
yaml-files = ["*.yaml"]

[rules]
document-start = "disable"
"#,
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let value = toml_config_to_value(&parsed);
    let document_start = value
        .get("rules")
        .and_then(|rules| rules.get("document-start"))
        .and_then(toml::Value::as_str);

    assert_eq!(document_start, Some("disable"));
}

#[test]
fn typed_toml_parser_accepts_toml_only_quoted_strings_option() {
    let parsed = parse_toml_config_str(
        "[rules.quoted-strings]\nallow-double-quotes-for-escaping = true\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    validate_toml_config(&parsed)
        .expect("TOML-only quoted-strings option should validate");
}

#[test]
fn typed_toml_to_value_handles_configs_without_rules() {
    let parsed = parse_toml_config_str("locale = \"en_US.UTF-8\"\n", false)
        .expect("typed TOML parse should succeed")
        .expect("project TOML should produce config");

    let value = toml_config_to_value(&parsed);
    assert_eq!(
        value.get("locale").and_then(toml::Value::as_str),
        Some("en_US.UTF-8")
    );
    assert!(value.get("rules").is_none());
}

#[test]
fn typed_toml_parser_round_trips_unknown_extras_and_custom_rules() {
    let parsed = parse_toml_config_str(
        r#"
flag = true
stamp = 1979-05-27T07:32:00Z

[extra]
name = "demo"

[rules]
anchors = "disable"

[rules.custom-rule]
count = 3
stamp = 1979-05-27T07:32:00Z
"#,
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let value = toml_config_to_value(&parsed);
    assert_eq!(value.get("flag").and_then(toml::Value::as_bool), Some(true));
    assert!(
        value
            .get("stamp")
            .and_then(toml::Value::as_datetime)
            .is_some()
    );
    assert_eq!(
        value
            .get("extra")
            .and_then(|extra| extra.get("name"))
            .and_then(toml::Value::as_str),
        Some("demo")
    );
    assert_eq!(
        value
            .get("rules")
            .and_then(|rules| rules.get("custom-rule"))
            .and_then(|rule| rule.get("count"))
            .and_then(toml::Value::as_integer),
        Some(3)
    );
    assert!(
        value
            .get("rules")
            .and_then(|rules| rules.get("custom-rule"))
            .and_then(|rule| rule.get("stamp"))
            .and_then(toml::Value::as_datetime)
            .is_some()
    );
}

#[test]
fn typed_toml_parser_extracts_tool_ryl_from_pyproject() {
    let parsed = parse_toml_config_str(
        r#"
[project]
name = "demo"
version = "0.1.0"

[tool.ryl]
locale = "it_IT.UTF-8"
"#,
        true,
    )
    .expect("typed pyproject parse should succeed")
    .expect("tool.ryl should be present");

    assert_eq!(parsed.locale.as_deref(), Some("it_IT.UTF-8"));
}

#[test]
fn normalize_toml_config_flattens_top_level_fields_and_rules() {
    let parsed = parse_toml_config_str(
        r#"
ignore = "vendor/**"
yaml-files = ["*.yaml"]

[fix]
unfixable = ["comments"]

[rules.comments]
level = "warning"
require-starting-space = true
"#,
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let normalized = normalize_toml_config(&parsed);

    assert_eq!(
        normalized.ignore_patterns,
        Some(vec!["vendor/**".to_string()])
    );
    assert_eq!(
        normalized.yaml_file_patterns,
        Some(vec!["*.yaml".to_string()])
    );
    assert_eq!(
        normalized
            .fix
            .as_ref()
            .expect("fix should normalize")
            .unfixable
            .len(),
        1
    );
    assert!(normalized.rules.contains_key("comments"));
}

#[test]
fn normalized_config_to_toml_value_skips_yaml_files_when_absent() {
    let value = normalized_config_to_toml_value(&NormalizedConfig {
        ignore_patterns: Some(vec!["vendor/**".to_string()]),
        ..NormalizedConfig::default()
    });

    let table = value.as_table().expect("config should serialize as table");
    assert!(!table.contains_key("yaml-files"));
    assert_eq!(
        table
            .get("ignore")
            .and_then(toml::Value::as_array)
            .expect("ignore should serialize as array")
            .len(),
        1
    );
}

#[test]
fn typed_toml_parser_returns_none_for_missing_pyproject_section() {
    let parsed = parse_toml_config_str(
        r#"
[project]
name = "demo"
version = "0.1.0"
"#,
        true,
    )
    .expect("typed pyproject parse should succeed");

    assert!(parsed.is_none());
}

#[test]
fn typed_toml_parser_errors_for_invalid_project_toml() {
    let err = parse_toml_config_str("rules = [", false)
        .expect_err("invalid TOML should error");

    assert!(err.contains("failed to parse config data:"));
}

#[test]
fn typed_toml_parser_errors_for_invalid_pyproject_toml() {
    let err = parse_toml_config_str("[tool.ryl]\nrules = [", true)
        .expect_err("invalid pyproject TOML should error");

    assert!(err.contains("failed to parse config data:"));
}

#[test]
fn typed_toml_validation_rejects_ignore_and_ignore_from_file_together() {
    let parsed = parse_toml_config_str(
        "ignore = ['vendor/**']\nignore-from-file = ['.ignore-list']\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let err = validate_toml_config(&parsed)
        .expect_err("typed validation should reject conflicting ignore settings");

    assert_eq!(
        err,
        "invalid config: ignore and ignore-from-file keys cannot be used together"
    );
}

#[test]
fn typed_toml_validation_rejects_invalid_key_ordering_regex() {
    let parsed =
        parse_toml_config_str("[rules.key-ordering]\nignored-keys = ['[']\n", false)
            .expect("typed TOML parse should succeed")
            .expect("project TOML should produce config");

    let err = validate_toml_config(&parsed)
        .expect_err("typed validation should reject invalid regex");

    assert!(
        err.contains("invalid config: option \"ignored-keys\" of \"key-ordering\" contains invalid regex '[':"),
        "unexpected message: {err}"
    );
}

#[test]
fn typed_toml_validation_accepts_key_ordering_without_ignored_keys() {
    let parsed =
        parse_toml_config_str("[rules.key-ordering]\nlevel = 'warning'\n", false)
            .expect("typed TOML parse should succeed")
            .expect("project TOML should produce config");

    validate_toml_config(&parsed)
        .expect("typed validation should allow key-ordering without regexes");
}

#[test]
fn typed_toml_validation_accepts_valid_key_ordering_regex() {
    let parsed =
        parse_toml_config_str("[rules.key-ordering]\nignored-keys = ['^ok$']\n", false)
            .expect("typed TOML parse should succeed")
            .expect("project TOML should produce config");

    validate_toml_config(&parsed)
        .expect("typed validation should accept valid key-ordering regexes");
}

#[test]
fn typed_toml_validation_rejects_quoted_strings_conflicts() {
    let parsed = parse_toml_config_str(
        "[rules.quoted-strings]\nextra-required = ['^http']\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let err = validate_toml_config(&parsed).expect_err(
        "typed validation should reject conflicting quoted-strings options",
    );

    assert_eq!(
        err,
        "invalid config: quoted-strings: cannot use both \"required: true\" and \"extra-required\""
    );
}

#[test]
fn typed_toml_validation_rejects_required_true_with_extra_allowed() {
    let parsed = parse_toml_config_str(
        "[rules.quoted-strings]\nrequired = true\nextra-allowed = ['^http']\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let err = validate_toml_config(&parsed)
        .expect_err("typed validation should reject required true with extra-allowed");

    assert_eq!(
        err,
        "invalid config: quoted-strings: cannot use both \"required: true\" and \"extra-allowed\""
    );
}

#[test]
fn typed_toml_validation_rejects_required_false_with_extra_allowed() {
    let parsed = parse_toml_config_str(
        "[rules.quoted-strings]\nrequired = false\nextra-allowed = ['^http']\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let err = validate_toml_config(&parsed)
        .expect_err("typed validation should reject required false with extra-allowed");

    assert_eq!(
        err,
        "invalid config: quoted-strings: cannot use both \"required: false\" and \"extra-allowed\""
    );
}

#[test]
fn typed_toml_validation_rejects_invalid_quoted_strings_regex() {
    let parsed = parse_toml_config_str(
        "[rules.quoted-strings]\nrequired = false\nextra-required = ['[']\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let err = validate_toml_config(&parsed)
        .expect_err("typed validation should reject invalid quoted-strings regex");

    assert!(
        err.starts_with(
            "invalid config: regex \"[\" in option \"extra-required\" of \"quoted-strings\" is invalid:"
        ),
        "unexpected message: {err}"
    );
}

#[test]
fn typed_toml_validation_rejects_invalid_extra_allowed_regex() {
    let parsed = parse_toml_config_str(
        "[rules.quoted-strings]\nrequired = 'only-when-needed'\nextra-allowed = ['[']\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    let err = validate_toml_config(&parsed)
        .expect_err("typed validation should reject invalid extra-allowed regex");

    assert!(
        err.starts_with(
            "invalid config: regex \"[\" in option \"extra-allowed\" of \"quoted-strings\" is invalid:"
        ),
        "unexpected message: {err}"
    );
}

#[test]
fn typed_toml_validation_accepts_only_when_needed_without_regex_lists() {
    let parsed = parse_toml_config_str(
        "[rules.quoted-strings]\nrequired = 'only-when-needed'\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    validate_toml_config(&parsed).expect(
        "typed validation should allow only-when-needed without extra regex lists",
    );
}

#[test]
fn typed_toml_validation_accepts_valid_extra_allowed_regex() {
    let parsed = parse_toml_config_str(
        "[rules.quoted-strings]\nrequired = 'only-when-needed'\nextra-allowed = ['^cmd$']\n",
        false,
    )
    .expect("typed TOML parse should succeed")
    .expect("project TOML should produce config");

    validate_toml_config(&parsed)
        .expect("typed validation should allow valid extra-allowed regexes");
}
