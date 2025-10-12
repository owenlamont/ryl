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

#[test]
fn disable_line_inline_comment_suppresses_truthy() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "value: yes  # yamllint disable-line rule:truthy\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "inline disable-line should suppress diagnostics: {hits:?}"
    );
}

#[test]
fn disable_line_without_rule_applies_to_next_line() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "# yamllint disable-line\nvalue: on\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "global disable-line should apply to the next line: {hits:?}"
    );
}

#[test]
fn disable_line_with_other_rule_does_not_affect_truthy() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "value: on  # yamllint disable-line rule:comments\n";
    let hits = truthy::check(input, &resolved);
    assert_eq!(hits.len(), 1, "truthy diagnostics should remain: {hits:?}");
}

#[test]
fn disable_line_without_rule_list_still_matches_truthy() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "#   yamllint disable-line   \nvalue: off\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "implicit disable-line should apply to truthy rule: {hits:?}"
    );
}

#[test]
fn disable_line_with_multiple_rules_including_truthy() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "value: on  # yamllint disable-line rule:comments rule:truthy\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "listing truthy rule should suppress diagnostics: {hits:?}"
    );
}

#[test]
fn disable_line_without_rule_prefix_disables_truthy() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "value: on  # yamllint disable-line truthy\n";
    let hits = truthy::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "directive without rule: prefix should disable all rules: {hits:?}"
    );
}

#[test]
fn disable_line_parsing_handles_quotes_and_escapes() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let double_quoted = "value: \"hash # fragment\"  # yamllint disable-line rule:truthy\n";
    assert!(truthy::check(double_quoted, &resolved).is_empty());

    let single_quoted = "value: 'path\\#fragment'  # yamllint disable-line rule:truthy\n";
    assert!(truthy::check(single_quoted, &resolved).is_empty());

    let escaped_hash = "value: foo \\# not comment\n";
    assert!(truthy::check(escaped_hash, &resolved).is_empty());
}

#[test]
fn regular_comment_does_not_disable_truthy() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "value: yes  # regular comment\n";
    let hits = truthy::check(input, &resolved);
    assert_eq!(
        hits.len(),
        1,
        "non-directive comment should not disable rule"
    );
}

#[test]
fn disable_line_block_comment_with_truthy() {
    let resolved = build_config("rules:\n  truthy: enable\n");
    let input = "# yamllint disable-line rule:truthy\nvalue: yes\nnext: On\n";
    let hits = truthy::check(input, &resolved);
    assert_eq!(
        hits.len(),
        1,
        "only the immediately following line should be disabled"
    );
    assert_eq!(hits[0].line, 3);
}
