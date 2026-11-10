use std::path::Path;
use std::sync::LazyLock;

use proptest::prelude::*;
use ryl::yaml_dom::{ScalarOwned, YamlOwned};
use serde_json::{Value, json};

use crate::property::{check_strategy, strategy};

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: crate::property::failure_persistence!("property_schema"),
        ..ProptestConfig::default()
    })]

    #[test]
    fn rewrites_preserve_each_document_schema_selection(
        headers in prop::collection::vec(prop::sample::select(vec!["# $schema: none", "# yaml-language-server: $schema=./schema.json", "#yaml-language-server: $schema=deny.json", "# yaml-language-server:", "#\u{a0}$schema:\u{a0}none"]), 1..5),
        marker in prop::sample::select(vec!["add", "remove", "preserve"]),
        ending in prop::sample::select(vec!["\n", "\r\n", "\r"]),
    ) {
        let input = headers.iter().map(|header| format!("{header}{ending}z:  1{ending}a: 2{ending}")).collect::<Vec<_>>().join(&format!("---{ending}"));
        let cfg = ryl::config::YamlLintConfig::from_toml_str(&format!("[format]\ndocument-start = '{marker}'\n[lint.rules.schema]\ndirectives = true\n[lint.rules.key-ordering]\n[lint.rules.comments]\n")).unwrap();
        for rewrite in [ryl::fix::Rewrite::Fix, ryl::fix::Rewrite::Format] {
            let output = ryl::fix::rewrite_str(&input, &cfg, Path::new("in.yaml"), Path::new("."), ryl::config::SourceKind::Yaml, rewrite).0.unwrap_or_else(|| input.clone());
            prop_assert_eq!(ryl::rules::schema::document_directives(&input), ryl::rules::schema::document_directives(&output));
            prop_assert!(ryl::fix::rewrite_str(&output, &cfg, Path::new("in.yaml"), Path::new("."), ryl::config::SourceKind::Yaml, rewrite).0.is_none());
        }
    }
}

fn json_value(node: &YamlOwned) -> Option<Value> {
    match node {
        YamlOwned::Value(ScalarOwned::Null) => Some(Value::Null),
        YamlOwned::Value(ScalarOwned::Boolean(value)) => Some((*value).into()),
        YamlOwned::Value(ScalarOwned::Integer(value)) => Some((*value).into()),
        YamlOwned::Value(ScalarOwned::FloatingPoint(value)) => {
            serde_json::Number::from_f64(value.0).map(Value::Number)
        }
        YamlOwned::Value(ScalarOwned::String(value)) => {
            if value == "<<" || ryl::yaml_dom::is_core_schema_int_spelling(value) {
                None
            } else {
                Some(value.clone().into())
            }
        }
        YamlOwned::Sequence(values) => values
            .iter()
            .map(json_value)
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        YamlOwned::Mapping(values) => values
            .iter()
            .map(|(key, value)| {
                let key = key.as_str()?;
                if key == "<<" {
                    return None;
                }
                Some((key.to_owned(), json_value(value)?))
            })
            .collect::<Option<serde_json::Map<_, _>>>()
            .map(Value::Object),
        _ => None,
    }
}

fn check_overlap(source: &str) {
    if source.contains("%YAML 1.1") {
        return;
    }
    let Ok(documents) = YamlOwned::load_from_str(source) else {
        return;
    };
    let [document] = documents.as_slice() else {
        return;
    };
    let Some(expected) = json_value(document) else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let ctx = crate::cli_schema_rule::config(dir.path(), &json!({"const":expected}));
    let hits = ryl::rules::schema::check(source, &ctx.config, Path::new("in.yaml"));
    assert!(
        hits.iter()
            .all(|hit| hit.message.starts_with("cannot validate:")),
        "{source:?}: {hits:?}"
    );
}

fn check_spans(source: &str) {
    static CONFIG: LazyLock<(tempfile::TempDir, ryl::config::YamlLintConfig)> =
        LazyLock::new(|| {
            let dir = tempfile::tempdir().unwrap();
            let ctx = crate::cli_schema_rule::config(
                dir.path(),
                &json!({"additionalProperties":{"type":"string"},"items":{"type":"integer"}}),
            );
            (dir, ctx.config)
        });
    let hits = ryl::rules::schema::check(source, &CONFIG.1, Path::new("in.yaml"));
    let spans = hits
        .into_iter()
        .map(|hit| crate::property::check_harness::Span {
            rule: "schema",
            line: hit.line,
            column: hit.column,
        })
        .collect::<Vec<_>>();
    crate::property::check_harness::check_spans_in_bounds(source, &spans).unwrap();
}

fn check_versioned_projection(yaml11: bool, value: u16, tag: &str) {
    let version = if yaml11 { "%YAML 1.1\n---\n" } else { "" };
    let spelling = if tag == "float" {
        format!("0{value}")
    } else {
        value.to_string()
    };
    let yaml = format!(
        "{version}&m !!merge <<: {{}}\njob: {{*m : {{value: !!{tag} {spelling}}}}}\n"
    );
    let hits = crate::cli_schema_rule::findings(
        &json!({"const":{"job":{"value":value}}}),
        &yaml,
    );
    assert!(hits.is_empty(), "{yaml}: {hits:?}");
    let hits = crate::cli_schema_rule::findings(
        &json!({"anyOf":[{"properties":{"job":{"properties":{"value":{"type":"string"}}}}}, {"type":"array"}]}),
        &yaml,
    );
    assert_eq!(hits.len(), 1, "{yaml}: {hits:?}");
    assert_eq!(hits[0].message, "value is not of type \"string\"");
    assert_eq!(hits[0].line, if yaml11 { 4 } else { 2 });
    let hits = crate::cli_schema_rule::findings(
        &json!(true),
        &format!("{version}!!{tag} true\n"),
    );
    assert_eq!(hits[0].message, "cannot validate: invalid scalar tag");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: crate::property::failure_persistence!("property_schema"),
        ..ProptestConfig::default()
    })]

    #[test]
    fn projection_matches_dom_on_core_overlap(document in strategy::arb_document()) {
        check_overlap(&document.render());
    }

    #[test]
    fn diagnostics_have_character_aligned_in_bounds_spans(document in check_strategy::arb_document()) {
        check_spans(&document.render());
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        failure_persistence: crate::property::failure_persistence!("property_schema"),
        ..ProptestConfig::default()
    })]

    #[test]
    fn versioned_merges_tags_and_alternative_diagnostics(
        yaml11 in any::<bool>(), value in any::<u16>(), tag in prop_oneof![Just("int"), Just("float")],
    ) {
        check_versioned_projection(yaml11, value, tag);
    }
}

#[test]
fn projection_oracle_and_span_checks_have_deterministic_witnesses() {
    check_overlap("flag: true\nnumber: 1.5\nlist: [null, 2, 'text']\n");
    check_spans("emoji: 😀\n'a/b~c': [1, 2]\n");
    check_versioned_projection(false, 42, "int");
    check_versioned_projection(true, 42, "float");
}

fn check_recursive_branches(names: Vec<String>, mode: u8) {
    let (reference, metadata) = match mode {
        0 => (json!({"$ref":"#"}), json!({})),
        1 => (
            json!({"$recursiveRef":"#"}),
            json!({"$schema":"https://json-schema.org/draft/2019-09/schema", "$recursiveAnchor":true}),
        ),
        2 => (
            json!({"$recursiveRef":"#/$defs/leaf"}),
            json!({"$schema":"https://json-schema.org/draft/2019-09/schema", "$defs":{"leaf":true}}),
        ),
        _ => (
            json!({"$dynamicRef":"#node"}),
            json!({"$dynamicAnchor":"node"}),
        ),
    };
    let dir = tempfile::tempdir().unwrap();
    let properties = names
        .into_iter()
        .map(|name| (name, reference.clone()))
        .collect::<serde_json::Map<_, _>>();
    let mut schema = metadata;
    schema["properties"] = json!(properties);
    let ctx = crate::cli_schema_rule::config(dir.path(), &schema);
    assert!(
        ryl::rules::schema::check("{}\n", &ctx.config, Path::new("in.yaml")).is_empty()
    );
    schema["allOf"] = json!([
        {"properties":{"x":reference}}, {"properties":{"x":reference}}
    ]);
    std::fs::write(dir.path().join("schema.json"), schema.to_string()).unwrap();
    let error = ryl::config::discover_config(
        &[],
        &ryl::config::Overrides {
            config_file: Some(dir.path().join(".ryl.toml")),
            config_data: None,
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("overlapping recursive branches"),
        "{error}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: crate::property::failure_persistence!("property_schema"),
        ..ProptestConfig::default()
    })]

    #[test]
    fn distinct_recursive_properties_are_accepted(names in prop::collection::vec("[a-z/~]{1,8}", 1..8), mode in 0_u8..4) {
        check_recursive_branches(names, mode);
    }
}

#[test]
fn escaped_recursive_properties_have_a_deterministic_witness() {
    for mode in 0..4 {
        check_recursive_branches(vec!["a/b".to_owned(), "~".to_owned()], mode);
    }
}

fn check_collection_tags_and_property_names(value: u16, key: &str) {
    let quoted_key = serde_json::to_string(key).unwrap();
    for (schema, yaml) in [
        (json!({"const":[value]}), format!("! [{value}]\n")),
        (
            json!({"const":{key:value}}),
            format!("! {{{quoted_key}: {value}}}\n"),
        ),
    ] {
        assert!(crate::cli_schema_rule::findings(&schema, &yaml).is_empty());
    }
    for node in [
        value.to_string(),
        format!("[{value}]"),
        format!("{{{quoted_key}: {value}}}"),
    ] {
        let yaml = format!("%TAG !e! tag:example.com,2020:\n---\n!e!%21 {node}\n");
        let hits = crate::cli_schema_rule::findings(&json!(true), &yaml);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].message.starts_with("cannot validate: tag"));
    }
    let yaml = format!("good: 0\n{quoted_key}: {value}\n");
    let hits = crate::cli_schema_rule::findings(
        &json!({"propertyNames":{"const":"good"}}),
        &yaml,
    );
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].line, hits[0].column), (2, 1));
    let hits = crate::cli_schema_rule::findings(
        &json!({"properties":{"good":true},"unevaluatedProperties":false}),
        &yaml,
    );
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].line, hits[0].column), (2, 1));
    let hits = crate::cli_schema_rule::findings(
        &json!({"prefixItems":[true],"contains":{"type":"null"},"unevaluatedItems":false}),
        &format!("- {value}\n- {value}\n- null\n- 2\n"),
    );
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].line, hits[0].column), (2, 3));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: crate::property::failure_persistence!("property_schema"),
        ..ProptestConfig::default()
    })]

    #[test]
    fn collection_tags_and_property_name_locations(value in any::<u16>(), key in "[A-Z/~]{1,8}") {
        check_collection_tags_and_property_names(value, &key);
    }
}

#[test]
fn collection_tags_and_property_name_locations_have_a_deterministic_witness() {
    check_collection_tags_and_property_names(42, "A/~B");
}

pub(crate) fn check_legacy_reference_siblings(draft: &str, id: &str, identifier: &str) {
    let schema = json!({
        "$schema":format!("http://json-schema.org/draft-{draft}/schema#"),
        "$ref":"#/definitions/entry", "allOf":[{"$ref":"#"}],
        "definitions":{"entry":{"properties":{"x":{id:identifier,"type":"string"}}}}
    });
    let hits = crate::cli_schema_rule::findings(&schema, "x: 1\n");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message, "value is not of type \"string\"");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: crate::property::failure_persistence!("property_schema"),
        ..ProptestConfig::default()
    })]

    #[test]
    fn legacy_reference_siblings_and_child_identifiers_match_validation(
        draft in prop_oneof![Just(("04", "id")), Just(("06", "$id")), Just(("07", "$id"))],
        identifier in "[a-z]{1,12}",
    ) {
        check_legacy_reference_siblings(draft.0, draft.1, &identifier);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: crate::property::failure_persistence!("property_schema"),
        ..ProptestConfig::default()
    })]

    #[test]
    fn boolean_resources_in_alternatives_have_stable_diagnostics(
        name in "[a-z]{1,8}", keyword in prop_oneof![Just("anyOf"), Just("oneOf")],
    ) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(format!("{name}[1].json")), "false").unwrap();
        let schema = json!({keyword:[{"$ref":format!("{name}%5B1%5D.json")},{"type":"string"}]});
        let ctx = crate::cli_schema_rule::config(dir.path(), &schema);
        let hits = ryl::rules::schema::check("{}\n", &ctx.config, Path::new("in.yaml"));
        prop_assert_eq!(hits.len(), 1);
        prop_assert!(hits[0].message.contains(keyword));
    }
}
