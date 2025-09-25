use ryl::config::YamlLintConfig;
use ryl::rules::quoted_strings::{self, Config};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config should parse");
    Config::resolve(&cfg)
}

#[test]
fn required_true_flags_plain_values() {
    let cfg = build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
    let hits = quoted_strings::check("foo: bar\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 6);
    assert_eq!(
        hits[0].message,
        "string value is not quoted with any quotes"
    );
}

#[test]
fn quote_type_single_requires_single_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n",
    );
    let hits = quoted_strings::check("foo: \"bar\"\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].message,
        "string value is not quoted with single quotes"
    );
}

#[test]
fn non_string_plain_values_are_ignored() {
    let cfg = build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
    let hits = quoted_strings::check("foo: 123\n", &cfg);
    assert!(hits.is_empty(), "numeric scalars should be skipped");
}

#[test]
fn required_false_respects_extra_required() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: false\n    extra-required: ['^http']\n",
    );
    let hits = quoted_strings::check("- http://example.com\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message, "string value is not quoted");
}

#[test]
fn only_when_needed_flags_redundant_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let hits = quoted_strings::check("foo: \"bar\"\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].message,
        "string value is redundantly quoted with any quotes"
    );
}

#[test]
fn only_when_needed_respects_extra_allowed() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n    extra-allowed: ['^http']\n",
    );
    let hits = quoted_strings::check("foo: \"http://example\"\n", &cfg);
    assert!(hits.is_empty(), "quoted URL should be allowed");
}

#[test]
fn required_false_flags_mismatched_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: false\n    quote-type: single\n",
    );
    let hits = quoted_strings::check("foo: \"bar\"\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].message.contains("single quotes"));
}

#[test]
fn only_when_needed_extra_required_enforces_quoting() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n    extra-required: ['^foo']\n",
    );
    let hits = quoted_strings::check("foo: foo\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].message.contains("not quoted"));
}

#[test]
fn only_when_needed_flags_mismatched_quote_type() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n    quote-type: single\n",
    );
    let hits = quoted_strings::check("foo: \"bar\"\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].message.contains("single quotes"));
}

#[test]
fn only_when_needed_mismatched_quote_type_when_quotes_required() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n    quote-type: single\n",
    );
    let hits = quoted_strings::check("foo: \"!bar\"\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].message,
        "string value is not quoted with single quotes"
    );
}

#[test]
fn tagged_scalars_are_skipped() {
    let cfg = build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
    let hits = quoted_strings::check("foo: !!str yes\n", &cfg);
    assert!(
        hits.is_empty(),
        "explicitly tagged scalars should be ignored"
    );
}

#[test]
fn literal_block_is_ignored() {
    let cfg = build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
    let hits = quoted_strings::check("foo: |\n  line\n", &cfg);
    assert!(hits.is_empty(), "literal blocks are outside rule scope");
}

#[test]
fn double_quoted_non_printable_is_considered_needed() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let yaml = "foo: \"\u{0007}\"\n";
    let hits = quoted_strings::check(yaml, &cfg);
    assert!(hits.is_empty(), "non-printable characters require quotes");
}

#[test]
fn quoted_value_starting_with_bang_keeps_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let hits = quoted_strings::check("foo: \"!foo\"\n", &cfg);
    assert!(hits.is_empty(), "values starting with bang need quotes");
}

#[test]
fn required_false_allows_plain_strings_without_extras() {
    let cfg =
        build_config("rules:\n  document-start: disable\n  quoted-strings:\n    required: false\n");
    let hits = quoted_strings::check("foo: bar\n", &cfg);
    assert!(hits.is_empty(), "plain values should be allowed");
}

#[test]
fn required_false_respects_matching_quote_type() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: false\n    quote-type: double\n",
    );
    let hits = quoted_strings::check("foo: \"bar\"\n", &cfg);
    assert!(hits.is_empty(), "matching quotes should be permitted");
}

#[test]
fn complex_keys_do_not_suppress_value_diagnostics() {
    let cfg = build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
    let yaml = "? { key: value }\n: data\n";
    let hits = quoted_strings::check(yaml, &cfg);
    assert_eq!(hits.len(), 1, "expected value diagnostic, got: {:?}", hits);
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].column, 3);
    assert_eq!(
        hits[0].message,
        "string value is not quoted with any quotes"
    );
}

#[test]
fn allow_quoted_quotes_permits_mismatched_quotes_with_inner_quote() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n    allow-quoted-quotes: true\n",
    );
    let hits = quoted_strings::check("foo: 'bar\"baz'\n", &cfg);
    assert!(hits.is_empty(), "mismatched quoting should be permitted");
}

#[test]
fn check_keys_true_flags_keys() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n    check-keys: true\n    extra-required: ['[:]']\n",
    );
    let hits = quoted_strings::check("foo:bar: baz\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 1);
    assert_eq!(hits[0].message, "string key is not quoted");
}

#[test]
fn flow_context_retain_quotes_when_needed() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let hits = quoted_strings::check("items: [\"a,b\"]\n", &cfg);
    assert!(
        hits.is_empty(),
        "quotes are required in flow contexts containing commas"
    );
}

#[test]
fn flow_context_after_multibyte_key_retain_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let yaml = "\u{00E9}: [\"a,b\"]\n";
    let hits = quoted_strings::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "flow context after multibyte key should keep quotes"
    );
}

#[test]
fn multiline_backslash_requires_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let yaml = "foo: \"line1\\\n  line2\"\n";
    let hits = quoted_strings::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "backslash line continuations should require quotes"
    );
}

#[test]
fn multiline_flow_tokens_require_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let yaml = "foo: \"{ missing\"\n";
    let hits = quoted_strings::check(yaml, &cfg);
    assert!(hits.is_empty(), "unbalanced flow tokens should keep quotes");
}

#[test]
fn multiline_backslash_with_crlf_requires_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let yaml = "foo: \"line1\\\r\n  line2\"\n";
    let hits = quoted_strings::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "CRLF backslash continuations should require quotes"
    );
}

#[test]
fn multiline_empty_double_quoted_value_is_handled() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let yaml = "foo: \"\n\"\n";
    let hits = quoted_strings::check(yaml, &cfg);
    assert!(hits.is_empty(), "blank multi-line content should not panic");
}

#[test]
fn inner_double_quotes_are_preserved() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let yaml = "foo: \"\\\"bar\\\"\"\n";
    let hits = quoted_strings::check(yaml, &cfg);
    assert!(hits.is_empty(), "embedded quotes should keep outer quoting");
}
