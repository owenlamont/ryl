use ryl::config::YamlLintConfig;
use ryl::rules::indentation::{self, Config, IndentSequencesSetting, SpacesSetting, Violation};

fn config(spaces: SpacesSetting, indent_sequences: IndentSequencesSetting, multi: bool) -> Config {
    Config::new_for_tests(spaces, indent_sequences, multi)
}

fn parse_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config should parse");
    Config::resolve(&cfg)
}

#[test]
fn detects_unindented_sequence_in_mapping() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n- item\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 1,
            message: "wrong indentation: expected 2 but found 0".to_string(),
        }]
    );
}

#[test]
fn allows_unindented_sequence_when_disabled() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    let yaml = "root:\n- item\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn detects_indented_sequence_when_disabled() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    let yaml = "root:\n  - item\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 3,
            message: "wrong indentation: expected 0 but found 2".to_string(),
        }]
    );
}

#[test]
fn detects_over_indented_sequence_when_required() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n      - item\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 7,
            message: "wrong indentation: expected 2 but found 6".to_string(),
        }]
    );
}

#[test]
fn enforces_consistent_spacing() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n   child: value\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 4,
            message: "wrong indentation: expected 2 but found 3".to_string(),
        }]
    );
}

#[test]
fn checks_multiline_strings_when_enabled() {
    let cfg = config(SpacesSetting::Fixed(4), IndentSequencesSetting::True, true);
    let yaml = "quote: |\n    good\n     bad\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 3,
            column: 6,
            message: "wrong indentation: expected 4but found 5".to_string(),
        }]
    );
}

#[test]
fn multiline_strings_ignored_when_disabled() {
    let cfg = config(SpacesSetting::Fixed(4), IndentSequencesSetting::True, false);
    let yaml = "quote: |\n    good\n     bad\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn folded_multiline_reports_violation() {
    let cfg = config(SpacesSetting::Fixed(4), IndentSequencesSetting::True, true);
    let yaml = "quote: >\n    good\n     bad\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 3,
            column: 6,
            message: "wrong indentation: expected 4but found 5".to_string(),
        }]
    );
}

#[test]
fn consistent_spaces_detects_violation() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "root:\n  child:\n    grand: 1\n   bad: 2\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 4,
            column: 4,
            message: "wrong indentation: expected 4 but found 3".to_string(),
        }]
    );
}

#[test]
fn multiline_resets_context_after_block() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, true);
    let yaml = "quote: |\n  text\nnext: value\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn indent_sequences_consistent_detects_mixed_styles() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::Consistent,
        false,
    );
    let yaml = "root:\n- top\nanother:\n  - inner\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 4,
            column: 3,
            message: "wrong indentation: expected 0 but found 2".to_string(),
        }]
    );
}

#[test]
fn indent_sequences_whatever_allows_both_styles() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::Whatever,
        false,
    );
    let yaml = "root:\n- top\nanother:\n  - inner\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn tab_indentation_is_counted() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n\tchild: value\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 2,
            message: "wrong indentation: expected 2 but found 1".to_string(),
        }]
    );
}

#[test]
fn resolve_indent_sequences_from_string_values() {
    let cfg_whatever = parse_config("rules:\n  indentation:\n    indent-sequences: whatever\n");
    let yaml = "root:\n- top\n  - inner\n";
    assert!(indentation::check(yaml, &cfg_whatever).is_empty());

    let cfg_consistent = parse_config("rules:\n  indentation:\n    indent-sequences: consistent\n");
    let unindented = "root:\n- first\n- second\n";
    assert!(indentation::check(unindented, &cfg_consistent).is_empty());

    let mixed = "root:\n  - first\n- second\n";
    let hits = indentation::check(mixed, &cfg_consistent);
    assert_eq!(hits.len(), 1, "expected single violation: {hits:?}");
    assert_eq!(hits[0].line, 3);
    assert!(hits[0].message.contains("wrong indentation"));
}

#[test]
fn indentation_config_rejects_non_string_option_keys() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  indentation:\n    true: false\n")
        .expect_err("expected validation failure");
    assert!(
        err.contains("unknown option \"true\" for rule \"indentation\""),
        "unexpected error: {err}"
    );
}

#[test]
fn skips_blank_lines_and_top_level_sequence_entries() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "- first\n\nsecond\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn reports_misaligned_mapping_with_consistent_spacing() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "root:\n  child:\n    nested: 1\n   bad_child: 2\n   repeated: 3\n wrong: 4\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(hits.len(), 3, "unexpected diagnostics: {hits:?}");
    assert!(
        hits.iter()
            .any(|hit| hit.line == 4 && hit.message.contains("expected 4 but found 3"))
    );
    assert!(
        hits.iter()
            .any(|hit| hit.line == 5 && hit.message.contains("expected 2 but found 3"))
    );
    assert!(
        hits.iter()
            .any(|hit| hit.line == 6 && hit.message.contains("expected 2 but found 1"))
    );
}

#[test]
fn consistent_indent_sequences_observe_initial_style() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::Consistent,
        false,
    );
    let yaml = "root:\n  - first\n  - second\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn consistent_indent_sequences_detect_style_switch() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::Consistent,
        false,
    );
    let yaml = "root:\n  - first\n- second\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(hits.len(), 1, "expected single violation: {hits:?}");
    assert_eq!(hits[0].line, 3);
    assert!(hits[0].message.contains("expected 2 but found 0"));
}

#[test]
fn multiline_blocks_reuse_consistent_spacing() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        true,
    );
    let yaml = "|\n  ok\nsecond: |\n  ok\n bad\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(hits.len(), 1, "expected single violation: {hits:?}");
    assert_eq!(hits[0].line, 5);
    assert!(hits[0].message.contains("expected 2but found 1"));
}

#[test]
fn inline_structures_and_comments_preserve_mapping_detection() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  inline: { nested: [1, 2] } # trailing comment\n  escaped: \"quote \\\" inside\" # trailing\n  single: 'hash # inside' # trailing\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn fixed_spacing_reports_repeated_misalignments() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n child_one: value\n child_two: value\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(
        hits.iter()
            .any(|hit| hit.line == 2 && hit.message.contains("expected 2 but found 1"))
    );
    assert!(
        hits.iter()
            .any(|hit| hit.line == 3 && hit.message.contains("expected 0 but found 1"))
    );
}

#[test]
fn plain_scalar_contexts_are_tracked() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  nested:\n    value\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn top_level_indented_plain_scalar_is_permitted() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "  value\n    deeper\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn sequence_of_mappings_reports_incorrect_dedent() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "- key:\n    - nested\n- other\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "expected yamllint-compatible indent handling, got: {hits:?}"
    );
}

#[test]
fn complex_mapping_keys_are_classified_correctly() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  key\\with: value\n  'single_quote': value\n  \"double_quote\": value\n  {braced}: value\n  [bracketed]: value\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn sequence_plain_scalar_creates_other_context() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  - item\n    plain\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn colon_without_key_is_not_considered_mapping() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = ": value\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn consistent_spacing_records_initial_delta() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "root:\n  child:\n    grand: 1\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn sequence_indented_under_mapping_finds_parent() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  - valid\n  child: value\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn consistent_sequence_spacing_obeys_fixed_step() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::Consistent,
        false,
    );
    let yaml = "root:\n      - item\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 7,
            message: "wrong indentation: expected 2 but found 6".to_string(),
        }]
    );
}

#[test]
fn dash_prefixed_scalar_not_sequence_entry() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "-foo: bar\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn nested_mapping_sequence_resolves_parent_indent() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  child:\n    - item\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn nested_sequences_allow_shared_indent_in_consistent_mode() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::Consistent,
        false,
    );
    let yaml = "-\n  - item\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "nested sequence should not raise diagnostics: {hits:?}"
    );
}

#[test]
fn mapping_children_in_sequence_keep_alignment() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "-\n  key:\n    child: 1\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "mapping children aligned with sequence offset should be accepted: {hits:?}"
    );
}

#[test]
fn mapping_after_sequence_entry_retains_expected_indent() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "root:\n  - key:\n      child: 1\n  sibling: 2\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "sibling mapping should keep expected indentation: {hits:?}"
    );
}

#[test]
fn multi_level_nested_sequences_align_consistently() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::Consistent,
        false,
    );
    let yaml = "-\n  -\n    - item\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "deeply nested sequences should reuse consistent indentation: {hits:?}"
    );
}

#[test]
fn mapping_after_nested_sequence_is_accepted() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "root:\n  - key:\n      child: 1\n    sibling: 2\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(
        hits.is_empty(),
        "mapping following nested sequence should align: {hits:?}"
    );
}
