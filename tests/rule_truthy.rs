use ryl::config::YamlLintConfig;
use ryl::rules::truthy::{self, Config};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn flags_plain_truthy_values_in_values() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let hits = truthy::check("key: True\nother: yes\n", &resolved);
    assert_eq!(hits.len(), 2, "expected to flag both values");
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 6);
    assert_eq!(
        hits[0].message,
        "truthy value should be one of [false, true]"
    );
    assert_eq!(hits[1].line, 2);
    assert_eq!(hits[1].column, 8);
}

#[test]
fn skips_quoted_or_explicitly_tagged_values() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let hits = truthy::check(
        "---\nstring: \"True\"\nexplicit: !!str yes\nboolean: !!bool True\n",
        &resolved,
    );
    assert!(hits.is_empty(), "quoted/tagged values should be ignored");
}

#[test]
fn respects_allowed_values_override() {
    let resolved = build_config("rules:\n  truthy:\n    allowed-values: [\"yes\", \"no\"]\n");
    let hits = truthy::check("key: yes\nkey2: true\n", &resolved);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].column, 7);
    assert_eq!(hits[0].message, "truthy value should be one of [no, yes]");
}

#[test]
fn respects_yaml_version_directive() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "yes: 1\n...\n%YAML 1.2\n---\nyes: 2\n...\n%YAML 1.1\n---\nyes: 3\n";
    let hits = truthy::check(input, &resolved);
    assert_eq!(hits.len(), 2, "only YAML 1.1 documents should flag 'yes'");
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 1);
    assert_eq!(hits[1].line, 9);
    assert_eq!(hits[1].column, 1);
}

#[test]
fn skips_keys_when_disabled() {
    let resolved =
        build_config("rules:\n  truthy:\n    allowed-values: []\n    check-keys: false\n");
    let hits = truthy::check("True: yes\nvalue: True\n", &resolved);
    assert_eq!(hits.len(), 2, "keys should be skipped but values flagged");
    assert!(
        hits.iter().all(|hit| !(hit.line == 1 && hit.column == 1)),
        "key diagnostics should be suppressed: {hits:?}"
    );
}

#[test]
fn flags_keys_when_enabled() {
    let resolved = build_config("rules:\n  truthy:\n    allowed-values: []\n");
    let hits = truthy::check("True: yes\n", &resolved);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 1);
    assert_eq!(hits[1].line, 1);
    assert_eq!(hits[1].column, 7);
}

#[test]
fn handles_complex_keys_without_leaking_key_depth() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "? { mixed: True }\n: value\n";
    let hits = truthy::check(input, &resolved);
    assert_eq!(hits.len(), 1, "should flag nested truthy value once");
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 12);
}

#[test]
fn ignores_malformed_yaml_directive_without_version() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "%YAML\n---\nfoo: True\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "malformed directive should be skipped: {hits:?}"
    );
}

#[test]
fn ignores_yaml_directive_with_non_numeric_version() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "%YAML 1.x\n---\nfoo: True\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "invalid directives should be ignored: {hits:?}"
    );
}

#[test]
fn ignores_yaml_directive_missing_minor_version() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "%YAML 1\n---\nfoo: True\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "directive without minor version should be ignored"
    );
}

#[test]
fn ignores_yaml_directive_with_non_numeric_major() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "%YAML x.1\n---\nfoo: True\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "directive with invalid major should be ignored"
    );
}
