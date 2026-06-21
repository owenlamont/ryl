use ryl::config::YamlLintConfig;
use ryl::rules::quoted_strings::{self, Config};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config should parse");
    Config::resolve(&cfg)
}

#[test]
fn required_true_flags_plain_values() {
    let cfg =
        build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
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
fn quote_type_consistent_uses_first_quoted_style() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: consistent\n",
    );
    let hits = quoted_strings::check("first: 'one'\nsecond: \"two\"\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].column, 9);
    assert_eq!(
        hits[0].message,
        "string value is not quoted with consistent quotes"
    );
}

#[test]
fn quote_type_consistent_ignores_plain_scalars_for_style_choice() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: false\n    quote-type: consistent\n",
    );
    let hits =
        quoted_strings::check("plain: value\nfirst: \"one\"\nsecond: 'two'\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 3);
    assert_eq!(hits[0].column, 9);
    assert_eq!(
        hits[0].message,
        "string value is not quoted with consistent quotes"
    );
}

#[test]
fn quote_type_consistent_keeps_style_across_documents() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: consistent\n",
    );
    let hits = quoted_strings::check("---\nfirst: 'one'\n---\nsecond: \"two\"\n", &cfg);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, 4);
    assert_eq!(hits[0].column, 9);
    assert_eq!(
        hits[0].message,
        "string value is not quoted with consistent quotes"
    );
}

#[test]
fn non_string_plain_values_are_ignored() {
    let cfg =
        build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
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
    let cfg =
        build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
    let hits = quoted_strings::check("foo: !!str yes\n", &cfg);
    assert!(
        hits.is_empty(),
        "explicitly tagged scalars should be ignored"
    );
}

#[test]
fn literal_block_is_ignored() {
    let cfg =
        build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
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
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: false\n",
    );
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
    let cfg =
        build_config("rules:\n  document-start: disable\n  quoted-strings: enable\n");
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

#[test]
fn fix_only_when_needed_removes_redundant_double_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: \"bar\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: bar\n"));
}

#[test]
fn fix_only_when_needed_removes_redundant_single_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: 'bar'\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: bar\n"));
}

#[test]
fn fix_only_when_needed_converts_double_to_single_when_needed() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: \"{value}\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: '{value}'\n"));
}

#[test]
fn fix_only_when_needed_keeps_single_quotes_when_needed_and_correct() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: '{value}'\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_only_when_needed_preserves_quotes_on_values_needing_escaping() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: \"a\\nb\"\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_required_always_adds_single_quotes_to_plain() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n",
    );
    let result = quoted_strings::fix("foo: bar\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: 'bar'\n"));
}

#[test]
fn fix_required_always_converts_double_to_single() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n",
    );
    let result = quoted_strings::fix("foo: \"bar\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: 'bar'\n"));
}

#[test]
fn fix_required_always_keeps_correct_single_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n",
    );
    let result = quoted_strings::fix("foo: 'bar'\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_required_always_adds_double_quotes_to_plain_when_configured() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n",
    );
    let result = quoted_strings::fix("foo: bar\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: \"bar\"\n"));
}

#[test]
fn fix_required_always_converts_single_to_double() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n",
    );
    let result = quoted_strings::fix("foo: 'bar'\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: \"bar\"\n"));
}

#[test]
fn fix_converts_double_to_single_escaping_inner_single_quote() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n",
    );
    let result = quoted_strings::fix("foo: \"it's\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: 'it''s'\n"));
}

#[test]
fn fix_removes_quotes_from_value_with_inner_single_quote_when_not_needed() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: 'it''s'\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: it's\n"));
}

#[test]
fn fix_consistent_converts_to_first_seen_style() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: consistent\n",
    );
    let result = quoted_strings::fix("first: 'one'\nsecond: \"two\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("first: 'one'\nsecond: 'two'\n"));
}

#[test]
fn fix_consistent_uses_later_existing_style_for_earlier_plain_scalar() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: consistent\n",
    );
    let result = quoted_strings::fix("plain: value\nquoted: \"two\"\n", &cfg);
    assert_eq!(
        result.as_deref(),
        Some("plain: \"value\"\nquoted: \"two\"\n")
    );
}

#[test]
fn fix_returns_none_when_no_changes_needed() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: bar\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_handles_multiple_scalars_in_one_pass() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("a: \"hello\"\nb: 'world'\nc: \"{flow}\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("a: hello\nb: world\nc: '{flow}'\n"));
}

#[test]
fn fix_required_never_does_not_remove_matching_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: false\n",
    );
    let result = quoted_strings::fix("foo: 'bar'\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_extra_allowed_permits_quotes_but_still_enforces_quote_type() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n    extra-allowed: ['^http']\n",
    );
    let result = quoted_strings::fix("foo: \"http://example.com\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: 'http://example.com'\n"));
}

#[test]
fn fix_respects_extra_required() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: false\n    extra-required: ['^http']\n",
    );
    let result = quoted_strings::fix("- http://example.com\n", &cfg);
    assert_eq!(result.as_deref(), Some("- 'http://example.com'\n"));
}

#[test]
fn fix_extra_allowed_converts_mismatched() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n    extra-allowed: ['^http']\n",
    );
    let result = quoted_strings::fix("foo: \"http://example.com\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: 'http://example.com'\n"));
}

#[test]
fn fix_only_when_needed_removes_quotes_when_value_is_plain_scalar_equivalent() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: \"http://example.com\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: http://example.com\n"));
}

#[test]
fn fix_skips_literal_block_scalar() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: |\n  literal\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_skips_tagged_scalar() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: !!str yes\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_required_never_with_extra_required_adds_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: false\n    extra-required: ['^must']\n",
    );
    let result = quoted_strings::fix("- must be quoted\n", &cfg);
    assert_eq!(result.as_deref(), Some("- 'must be quoted'\n"));
}

#[test]
fn fix_required_never_converts_mismatched_quote_type() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: false\n",
    );
    let result = quoted_strings::fix("foo: \"bar\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: 'bar'\n"));
}

#[test]
fn fix_only_when_needed_extra_required_adds_quotes_to_plain() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n    extra-required: ['foo']\n",
    );
    let result = quoted_strings::fix("- foo\n", &cfg);
    assert_eq!(result.as_deref(), Some("- 'foo'\n"));
}

#[test]
fn fix_only_when_needed_extra_allowed_no_mismatch_leaves_alone() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: any\n    required: only-when-needed\n    extra-allowed: ['^safe']\n",
    );
    let result = quoted_strings::fix("foo: \"safe-value\"\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_required_always_adds_double_quotes_with_quote_type_double() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n",
    );
    let result = quoted_strings::fix("foo: bar\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: \"bar\"\n"));
}

#[test]
fn fix_converts_single_to_double_escaping_inner_double_quote() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n",
    );
    let result = quoted_strings::fix("foo: 'he said \"hi\"'\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: \"he said \\\"hi\\\"\"\n"));
}

#[test]
fn fix_converts_single_to_double_escaping_backslash() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n",
    );
    let result = quoted_strings::fix("foo: 'path\\to'\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: \"path\\\\to\"\n"));
}

#[test]
fn fix_allow_quoted_quotes_skips_mismatch() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    allow-quoted-quotes: true\n",
    );
    let result = quoted_strings::fix("foo: \"he's\"\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_with_document_separator_still_fixes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("---\nfoo: \"bar\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("---\nfoo: bar\n"));
}

#[test]
fn fix_consistent_adds_quotes_to_plain_when_required_always() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: consistent\n",
    );
    let result = quoted_strings::fix("foo: bar\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: 'bar'\n"));
}

#[test]
fn fix_only_when_needed_consistent_extra_required_adds_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: consistent\n    required: only-when-needed\n    extra-required: ['must']\n",
    );
    let result = quoted_strings::fix("- must\n", &cfg);
    assert_eq!(result.as_deref(), Some("- 'must'\n"));
}

#[test]
fn fix_converts_single_to_double_with_tab_escaping() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n",
    );
    let result = quoted_strings::fix("foo: 'a\tb'\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: \"a\\tb\"\n"));
}

#[test]
fn fix_does_not_convert_non_printable_escape_to_single_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: false\n",
    );
    let result = quoted_strings::fix("foo: \"\\a\"\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_required_never_extra_required_adds_single_quotes_to_sequence_item() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: false\n    extra-required: ['^must']\n",
    );
    let result = quoted_strings::fix("- must\n", &cfg);
    assert_eq!(result.as_deref(), Some("- 'must'\n"));
}

#[test]
fn fix_required_never_plain_no_extra_returns_none() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: false\n",
    );
    let result = quoted_strings::fix("foo: bar\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_only_when_needed_plain_no_extra_returns_none() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: bar\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_preserves_escaped_double_quotes_when_option_set() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    )
    .with_allow_double_quotes_for_escaping(true);
    let result = quoted_strings::fix("foo: \"a\\nb\"\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_converts_unescaped_double_quotes_when_escaping_option_set() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    )
    .with_allow_double_quotes_for_escaping(true);
    let result = quoted_strings::fix("foo: \"bar\"\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: bar\n"));
}

#[test]
fn fix_only_when_needed_keeps_quotes_for_indicator_tokens() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let cases = [
        "cron: '30 21 * * 0'\n",
        "value: 'foo * bar'\n",
        "value: 'foo & bar'\n",
        "value: 'foo ! bar'\n",
        "value: 'foo | bar'\n",
        "value: 'foo > bar'\n",
        "value: 'foo ? bar'\n",
        "value: 'foo @ bar'\n",
        "value: 'foo % bar'\n",
        "value: 'foo ` bar'\n",
    ];

    for yaml in cases {
        let result = quoted_strings::fix(yaml, &cfg);
        assert_eq!(result.as_deref(), None, "quotes should stay for {yaml:?}");
    }
}

#[test]
fn only_when_needed_does_not_flag_indicator_token_content_as_redundant() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let cases = [
        "cron: '30 21 * * 0'\n",
        "value: 'foo & bar'\n",
        "value: 'foo ! bar'\n",
        "value: 'foo | bar'\n",
        "value: 'foo > bar'\n",
        "value: 'foo ? bar'\n",
        "value: 'foo @ bar'\n",
        "value: 'foo % bar'\n",
        "value: 'foo ` bar'\n",
    ];

    for yaml in cases {
        let hits = quoted_strings::check(yaml, &cfg);
        assert!(
            hits.is_empty(),
            "indicator token content should remain quoted: {yaml:?} => {hits:?}"
        );
    }
}

#[test]
fn fix_only_when_needed_preserves_inline_comments_when_unquoting() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: single\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: \"bar\" # trailing comment\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: bar # trailing comment\n"));
}

#[test]
fn fix_only_when_needed_ignores_unterminated_single_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: 'unterminated\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_only_when_needed_ignores_unterminated_double_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    required: only-when-needed\n",
    );
    let result = quoted_strings::fix("foo: \"unterminated\n", &cfg);
    assert_eq!(result.as_deref(), None);
}

#[test]
fn fix_escaping_exception_does_not_shield_single_quotes() {
    let cfg = build_config(
        "rules:\n  document-start: disable\n  quoted-strings:\n    quote-type: double\n",
    )
    .with_allow_double_quotes_for_escaping(true);
    let result = quoted_strings::fix("foo: 'bar'\n", &cfg);
    assert_eq!(result.as_deref(), Some("foo: \"bar\"\n"));
}

fn only_when_needed() -> Config {
    build_config("rules:\n  quoted-strings:\n    required: only-when-needed\n")
}

#[test]
fn keeps_quotes_on_yaml_1_1_ambiguous_scalars_under_explicit_1_1() {
    let cfg = only_when_needed();
    // Each resolves to a non-string under YAML 1.1 but a string under 1.2 core, so
    // dropping the quotes would change the value for a consumer honouring the directive.
    for value in [
        "no",
        "Yes",
        "y",
        "N",
        "0b101",
        "1_000",
        "1:30",
        "2002-12-14",
    ] {
        let input = format!("%YAML 1.1\n---\nkey: '{value}'\n");
        assert!(
            quoted_strings::check(&input, &cfg).is_empty(),
            "'{value}' must not be flagged redundant under explicit %YAML 1.1"
        );
        assert!(
            quoted_strings::fix(&input, &cfg).is_none(),
            "--fix must not strip '{value}' under explicit %YAML 1.1"
        );
    }
}

#[test]
fn strips_unambiguous_string_quotes_even_under_explicit_yaml_1_1() {
    let cfg = only_when_needed();
    let input = "%YAML 1.1\n---\nkey: 'hello'\n";
    assert_eq!(quoted_strings::check(input, &cfg).len(), 1);
    assert_eq!(
        quoted_strings::fix(input, &cfg).as_deref(),
        Some("%YAML 1.1\n---\nkey: hello\n"),
    );
}

#[test]
fn required_true_does_not_quote_a_yaml_1_1_boolean_under_explicit_1_1() {
    // `required: true` quotes string scalars, but under `%YAML 1.1` a plain `no` is the
    // boolean false, not a string, so quoting it would change the value.
    let cfg = build_config("rules:\n  quoted-strings:\n    required: true\n");
    let input = "%YAML 1.1\n---\nflag: no\n";
    assert!(quoted_strings::check(input, &cfg).is_empty());
    assert_eq!(quoted_strings::fix(input, &cfg), None);
}

#[test]
fn required_true_quotes_a_yaml_1_1_word_without_a_directive() {
    let cfg = build_config("rules:\n  quoted-strings:\n    required: true\n");
    assert_eq!(quoted_strings::check("flag: no\n", &cfg).len(), 1);
    assert_eq!(
        quoted_strings::fix("flag: no\n", &cfg).as_deref(),
        Some("flag: 'no'\n"),
    );
}

#[test]
fn strips_yaml_1_1_words_without_a_directive() {
    let cfg = only_when_needed();
    // Absent a directive ryl resolves under the 1.2 core schema, where `no` is a string,
    // so the quotes are genuinely redundant.
    assert_eq!(quoted_strings::check("key: 'no'\n", &cfg).len(), 1);
    assert_eq!(
        quoted_strings::fix("key: 'no'\n", &cfg).as_deref(),
        Some("key: no\n"),
    );
}

#[test]
fn strips_yaml_1_1_words_under_explicit_yaml_1_2() {
    let cfg = only_when_needed();
    let input = "%YAML 1.2\n---\nkey: 'no'\n";
    assert_eq!(quoted_strings::check(input, &cfg).len(), 1);
    assert_eq!(
        quoted_strings::fix(input, &cfg).as_deref(),
        Some("%YAML 1.2\n---\nkey: no\n"),
    );
}
