use ryl::config::YamlLintConfig;
use ryl::rules::key_duplicates::{self, Config};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn reports_duplicate_scalar_keys() {
    let cfg = build_config("rules:\n  key-duplicates: enable\n");
    let input = "first: 1\nsecond: 2\nfirst: 3\n";
    let hits = key_duplicates::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected single violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 3);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, "duplication of key \"first\" in mapping");
}

#[test]
fn reports_duplicate_flow_keys() {
    let cfg = build_config("rules:\n  key-duplicates: enable\n");
    let input = "{a: 1, b: 2, b: 3}\n";
    let hits = key_duplicates::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected single violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 1);
    assert_eq!(hit.column, 14);
    assert_eq!(hit.message, "duplication of key \"b\" in mapping");
}

#[test]
fn default_allows_merge_key_duplicates() {
    let cfg = build_config("rules:\n  key-duplicates: enable\n");
    let input = "anchor: &a\n  value: 1\nmerged:\n  <<: *a\n  <<: *a\n";
    let hits = key_duplicates::check(input, &cfg);
    assert!(hits.is_empty(), "merge keys allowed by default: {hits:?}");
}

#[test]
fn forbidding_merge_key_duplicates_reports_violation() {
    let cfg = build_config("rules:\n  key-duplicates:\n    forbid-duplicated-merge-keys: true\n");
    let input = "anchor: &a\n  value: 1\nmerged:\n  <<: *a\n  <<: *a\n";
    let hits = key_duplicates::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 5);
    assert_eq!(hit.column, 3);
    assert_eq!(hit.message, "duplication of key \"<<\" in mapping");
}

#[test]
fn nested_mappings_are_checked() {
    let cfg = build_config("rules:\n  key-duplicates: enable\n");
    let input = "---\n- inner: 1\n  inner: 2\n";
    let hits = key_duplicates::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected nested violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 3);
    assert_eq!(hit.column, 3);
    assert_eq!(hit.message, "duplication of key \"inner\" in mapping");
}

#[test]
fn complex_keys_are_ignored_without_panicking() {
    let cfg = build_config("rules:\n  key-duplicates: enable\n");
    let input = "---\n? [alpha, beta]\n: value\n";
    let hits = key_duplicates::check(input, &cfg);
    assert!(
        hits.is_empty(),
        "complex keys should not produce diagnostics: {hits:?}"
    );
}
