use std::process::Command;

use jsonschema::validator_for;
use ryl::config_schema::{
    normalize_toml_config, parse_toml_config_str, schema_value, toml_config_to_value,
    validate_toml_config,
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
