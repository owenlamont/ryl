use ryl::config::YamlLintConfig;
use ryl::rules::indentation::{
    self, Config, IndentSequencesSetting, SpacesSetting, Violation,
};

fn config(
    spaces: SpacesSetting,
    indent_sequences: IndentSequencesSetting,
    multi: bool,
) -> Config {
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
            message: "wrong indentation: expected 4 but found 5".to_string(),
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
            message: "wrong indentation: expected 4 but found 5".to_string(),
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
    let yaml = "root:\n  child:\n    grand: 1\n  other:\n      bad: 2\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 5,
            column: 7,
            message: "wrong indentation: expected 4 but found 6".to_string(),
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
fn resolve_indent_sequences_from_string_values() {
    let cfg_whatever =
        parse_config("rules:\n  indentation:\n    indent-sequences: whatever\n");
    let yaml = "root:\n- top\n  - inner\n";
    assert!(indentation::check(yaml, &cfg_whatever).is_empty());

    let cfg_consistent =
        parse_config("rules:\n  indentation:\n    indent-sequences: consistent\n");
    let unindented = "root:\n- first\n- second\n";
    assert!(indentation::check(unindented, &cfg_consistent).is_empty());

    let mixed = "a:\n  - first\nb:\n- second\n";
    let hits = indentation::check(mixed, &cfg_consistent);
    assert_eq!(hits.len(), 1, "expected single violation: {hits:?}");
    assert_eq!(hits[0].line, 4);
    assert!(hits[0].message.contains("wrong indentation"));
}

#[test]
fn indentation_config_rejects_non_string_option_keys() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  indentation:\n    true: false\n")
            .expect_err("expected validation failure");
    assert!(
        err.contains("cannot convert non-string TOML key"),
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
fn top_level_sequence_of_inline_mappings_is_allowed() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "- name: Foo\n- name: Bar\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn nested_inline_mapping_sequence_entries_share_parent_indent() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  - key: Foo\n  - key: Bar\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn reports_misaligned_mapping_with_consistent_spacing() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "root:\n  child:\n    nested: 1\n  sibling:\n     bad: 2\n  last:\n     also: 3\n";
    let hits = indentation::check(yaml, &cfg);
    let expected = |line| Violation {
        line,
        column: 6,
        message: "wrong indentation: expected 4 but found 5".to_string(),
    };
    assert_eq!(hits, vec![expected(5), expected(7)]);
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
    let yaml = "a:\n  - first\nb:\n- second\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 4,
            column: 1,
            message: "wrong indentation: expected 2 but found 0".to_string(),
        }]
    );
}

#[test]
fn multiline_blocks_reuse_consistent_spacing() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        true,
    );
    let yaml = "first: |\n  ok\nsecond: |\n  ok\n   bad\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 5,
            column: 4,
            message: "wrong indentation: expected 2 but found 3".to_string(),
        }]
    );
}

#[test]
fn inline_structures_and_comments_preserve_mapping_detection() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  inline: { nested: [1, 2] } # trailing comment\n  escaped: \"quote \\\" inside\" # trailing\n  single: 'hash # inside' # trailing\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn compact_flow_mapping_sequence_resets_after_dedent() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "root:\n  - {a: 1,\n     b: 2}\nnext: value\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn fixed_spacing_reports_only_the_first_misaligned_sibling() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n child_one: value\n child_two: value\n";
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
fn plain_scalar_contexts_are_tracked() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n  nested:\n    value\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn top_level_indented_plain_scalar_is_reported() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "  value\n    deeper\n";
    assert_eq!(
        indentation::check(yaml, &cfg),
        vec![Violation {
            line: 1,
            column: 3,
            message: "wrong indentation: expected 0 but found 2".to_string(),
        }]
    );
}

#[test]
fn sequence_of_mappings_can_dedent_to_root() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "- key:\n    - nested\n- other\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

#[test]
fn sequence_entry_mapping_requires_nested_sequence_indent_when_enabled() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "- key:\n  - nested\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 3,
            message: "wrong indentation: expected 4 but found 2".to_string(),
        }]
    );
}

#[test]
fn top_level_indented_sequence_is_reported_once() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "  - key: Foo\n  - key: Bar\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 3,
            message: "wrong indentation: expected 0 but found 2".to_string(),
        }]
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
fn value_without_key_cannot_infer_indentation() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let hits = indentation::check(": value\n", &cfg);
    assert_eq!(
        hits[0],
        Violation {
            line: 1,
            column: 1,
            message: "cannot infer indentation: unexpected token".to_string(),
        }
    );
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
    let yaml = "root:\n  - valid\nchild: value\n";
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
fn detects_unindented_mapping_sequence_in_mapping() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "metadata:\n  name: test\nsubjects:\n- apiGroup: rbac.authorization.k8s.io\n  kind: User\n  name: kubelet\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 4,
            column: 1,
            message: "wrong indentation: expected 2 but found 0".to_string(),
        }]
    );
}

#[test]
fn allows_indented_mapping_sequence_in_mapping() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "subjects:\n  - apiGroup: rbac.authorization.k8s.io\n    kind: User\n";
    assert!(indentation::check(yaml, &cfg).is_empty());
}

fn hits(yaml: &str, cfg: &Config) -> Vec<(usize, usize, String)> {
    indentation::check(yaml, cfg)
        .into_iter()
        .map(|hit| (hit.line, hit.column, hit.message))
        .collect()
}

fn wrong(line: usize, column: usize, expected: usize) -> (usize, usize, String) {
    let found = column - 1;
    (
        line,
        column,
        format!("wrong indentation: expected {expected} but found {found}"),
    )
}

// Expected diagnostics below were produced by yamllint 1.38 on the same input.
#[test]
fn explicit_keys_latch_the_step_from_their_content() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "? a\n: b\n?\n    k\n:\n    v\n";
    assert_eq!(hits(yaml, &cfg), vec![wrong(4, 5, 2), wrong(6, 5, 2)]);
    assert!(hits("?\n", &cfg).is_empty());
}

#[test]
fn multi_line_flow_collections_check_items_and_closers() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "a: [\n    1,\n  2,\n  ]\nb: {\n  c: 1\n}\n";
    assert_eq!(
        hits(yaml, &cfg),
        vec![wrong(3, 3, 4), wrong(4, 3, 0), wrong(6, 3, 4)]
    );
    assert!(hits("[x,\n a: b\n]\n", &cfg).is_empty());
}

#[test]
fn multi_line_scalars_follow_their_owner() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        true,
    );
    let yaml = "- |\n    x\n     y\n- ? >\n      k\n  : >\n      v\n- key:\n    |\n      a\n- plain\n  cont\n   more\n- 'q\n  r\n   s'\n- k: # c\n    |\n    t\n- ? k\n  : |\n     u\n";
    assert_eq!(
        hits(yaml, &cfg),
        vec![
            wrong(3, 6, 4),
            wrong(13, 4, 2),
            wrong(15, 3, 3),
            wrong(19, 5, 6),
            wrong(22, 6, 6),
        ]
    );
}

#[test]
fn unindented_sequence_before_the_step_is_known_expects_at_least_one_more() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    assert_eq!(
        hits("- a:\n  - x\n", &cfg),
        vec![(2, 3, "wrong indentation: expected at least 3".to_string())]
    );
}
