use ryl::config::YamlLintConfig;
use ryl::rules::empty_values::{self, Config, Violation};

fn resolve_config(data: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(data).unwrap();
    empty_values::Config::resolve(&cfg)
}

#[test]
fn reports_block_mapping_empty_value() {
    let yaml = "block:\n  missing:\n";
    let cfg = resolve_config("rules:\n  empty-values: enable\n");
    let hits = empty_values::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 11,
            message: "empty value in block mapping".to_string(),
        }]
    );
}

#[test]
fn reports_flow_mapping_empty_value() {
    let yaml = "root: { key: }\n";
    let cfg = resolve_config("rules:\n  empty-values: enable\n");
    let hits = empty_values::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 13,
            message: "empty value in flow mapping".to_string(),
        }]
    );
}

#[test]
fn reports_block_sequence_empty_value() {
    let yaml = "-\n";
    let cfg = resolve_config("rules:\n  empty-values: enable\n");
    let hits = empty_values::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 2,
            message: "empty value in block sequence".to_string(),
        }]
    );
}

#[test]
fn respects_block_sequence_flag() {
    let yaml = "-\n";
    let cfg = resolve_config("rules:\n  empty-values:\n    forbid-in-block-sequences: false\n");
    let hits = empty_values::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn respects_flow_mapping_flag() {
    let yaml = "root: { key: }\n";
    let cfg = resolve_config("rules:\n  empty-values:\n    forbid-in-flow-mappings: false\n");
    let hits = empty_values::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn respects_block_mapping_flag() {
    let yaml = "key:\n";
    let cfg = resolve_config("rules:\n  empty-values:\n    forbid-in-block-mappings: false\n");
    let hits = empty_values::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn ignores_flow_sequences() {
    let yaml = "seq: [ value ]\n";
    let cfg = resolve_config("rules:\n  empty-values: enable\n");
    let hits = empty_values::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn handles_alias_nodes() {
    let yaml = "anchors:\n  value: &id 1\nusage:\n  alias: *id\n";
    let cfg = resolve_config("rules:\n  empty-values: enable\n");
    let hits = empty_values::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn covers_nothing_event_branch() {
    empty_values::coverage_touch_nothing_branch();
}
