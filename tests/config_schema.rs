use std::process::Command;

use jsonschema::validator_for;
use ryl::config_schema::{parse_toml_config_str, schema_value, toml_config_to_value};
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
    let definition = reference
        .strip_prefix("#/$defs/")
        .expect("schema ref should point into $defs");

    schema
        .get("$defs")
        .and_then(|defs| defs.get(definition))
        .and_then(|entry| entry.get("properties"))
        .expect("schema definition properties should exist")
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
"#,
    );

    assert!(
        !validator.is_valid(&instance),
        "schema should reject invalid field types"
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
fn cli_print_config_schema_outputs_generated_schema_without_inputs() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let out = Command::new(exe)
        .arg("--print-config-schema")
        .output()
        .expect("schema command should run");

    assert!(
        out.status.success(),
        "schema command should succeed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout =
        String::from_utf8(out.stdout).expect("schema output should be valid UTF-8");
    let printed: serde_json::Value =
        serde_json::from_str(&stdout).expect("schema output should be JSON");

    assert_eq!(printed, schema_value());
    assert!(out.stderr.is_empty());
}

#[test]
fn generated_schema_serializes_as_json_schema_document() {
    let schema = schema_value();

    assert_eq!(schema.get("title"), Some(&json!("ryl TOML config")));
    assert!(schema.get("$schema").is_some());
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
