use std::path::Path;
use std::sync::LazyLock;

use proptest::prelude::*;
use ryl::yaml_dom::{ScalarOwned, YamlOwned};
use serde_json::{Value, json};

use crate::property::{check_strategy, strategy};

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

#[test]
fn projection_oracle_and_span_checks_have_deterministic_witnesses() {
    check_overlap("flag: true\nnumber: 1.5\nlist: [null, 2, 'text']\n");
    check_spans("emoji: 😀\n'a/b~c': [1, 2]\n");
}
