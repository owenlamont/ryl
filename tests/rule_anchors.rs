use ryl::rules::anchors::{
    self, Config, MESSAGE_AMBIGUOUS_ALIAS, MESSAGE_AMBIGUOUS_ANCHOR,
    MESSAGE_DUPLICATED_ANCHOR, MESSAGE_UNDECLARED_ALIAS, MESSAGE_UNUSED_ANCHOR,
    Violation,
};

fn ambiguous_cfg() -> Config {
    Config::new_for_tests(true, false, false)
        .with_forbid_ambiguous_anchor_alias_names(true)
}

fn violation(line: usize, column: usize, message: &str) -> Violation {
    Violation {
        line,
        column,
        message: message.to_string(),
    }
}

#[test]
fn empty_input_produces_no_diagnostics() {
    let cfg = Config::new_for_tests(true, true, true);
    let hits = anchors::check("", &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn handles_windows_line_endings() {
    let cfg = Config::new_for_tests(true, false, false);
    let yaml = "---\r\n- &anchor value\r\n- *anchor\r\n";
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn malformed_empty_anchor_defers_to_syntax_error() {
    // `& value` is an empty anchor name, a scanner error: the token stream stops
    // there, so the rule emits nothing and the alias after it is not reached. Lint
    // surfaces the granit syntax error instead of an anchors diagnostic.
    let cfg = Config::new_for_tests(true, true, true);
    let hits = anchors::check("---\n- & value\n- *missing\n", &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn allows_valid_usage() {
    let cfg = Config::new_for_tests(true, false, false);
    let yaml = "---\n- &anchor value\n- *anchor\n";
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn reports_undeclared_alias() {
    let cfg = Config::new_for_tests(true, false, false);
    let yaml = "---\n- *anchor\n- &anchor value\n";
    let hits = anchors::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![violation(
            2,
            3,
            &format!(r#"{MESSAGE_UNDECLARED_ALIAS} "anchor""#)
        )]
    );
}

#[test]
fn allows_forward_alias_when_disabled() {
    let cfg = Config::new_for_tests(false, false, false);
    let yaml = "---\n- *anchor\n- &anchor value\n";
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn reports_duplicate_anchor_when_enabled() {
    let cfg = Config::new_for_tests(false, true, false);
    let yaml = "---\n- &anchor first\n- &anchor second\n- *anchor\n";
    let hits = anchors::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![violation(
            3,
            3,
            &format!(r#"{MESSAGE_DUPLICATED_ANCHOR} "anchor""#)
        )]
    );
}

#[test]
fn reports_unused_anchor() {
    let cfg = Config::new_for_tests(false, false, true);
    let yaml = "---\n- &anchor value\n- 42\n";
    let hits = anchors::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![violation(
            2,
            3,
            &format!(r#"{MESSAGE_UNUSED_ANCHOR} "anchor""#)
        )]
    );
}

#[test]
fn alias_glued_to_colon_is_distinct_from_the_plain_anchor() {
    // Per the YAML spec / reference parser, `*x:` in `{*x: 2}` is the alias name
    // `x:` (the colon is a legal name char), distinct from anchor `x` — so the
    // alias is undeclared and the anchor unused. ryl follows the spec here; the
    // unambiguous, spec-valid alias-key form is `*x : 2`. (yamllint narrows to `x`.)
    let cfg = Config::new_for_tests(true, false, true);
    let hits = anchors::check("a: &x 1\nb: {*x: 2}\n", &cfg);
    assert_eq!(
        hits,
        vec![
            violation(2, 5, &format!(r#"{MESSAGE_UNDECLARED_ALIAS} "x:""#)),
            violation(1, 4, &format!(r#"{MESSAGE_UNUSED_ANCHOR} "x""#)),
        ]
    );
}

#[test]
fn duplicate_anchor_name_used_via_last_is_not_unused() {
    // An alias binds the latest declaration of the name, so the name counts as
    // used and `forbid-unused-anchors` reports nothing (matching yamllint's
    // name-keyed model); the shadowed first `&b` is the duplicated-anchors case.
    let cfg = Config::new_for_tests(false, false, true);
    let hits = anchors::check("- &b 1\n- &b 2\n- *b\n", &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn duplicate_unused_anchor_name_reported_once_at_last() {
    // Two unused declarations of one name report unused once, at the last
    // declaration (not once per record).
    let cfg = Config::new_for_tests(false, false, true);
    let hits = anchors::check("- &b 1\n- &b 2\n", &cfg);
    assert_eq!(
        hits,
        vec![violation(2, 3, &format!(r#"{MESSAGE_UNUSED_ANCHOR} "b""#))]
    );
}

#[test]
fn resets_state_between_documents() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "---\n",
        "- &anchor first\n",
        "- *anchor\n",
        "...\n",
        "---\n",
        "- &anchor second\n",
        "- 1\n"
    );
    let hits = anchors::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![violation(
            6,
            3,
            &format!(r#"{MESSAGE_UNUSED_ANCHOR} "anchor""#)
        )]
    );
}

#[test]
fn ignores_ampersand_in_strings_and_block_scalars() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "key: \"value &not\"\n",
        "quote: '&still not'\n",
        "literal: |\n",
        "  line with &amp\n",
        "folded: >\n",
        "  still not &anchor\n",
        "- &real anchor\n",
        "- *real\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn block_scalar_activation_and_release() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "block: |\n",
        "\n",
        "  &ignored anchor\n",
        "  still inside block\n",
        "next: 1\n",
        "- &real anchor\n",
        "- *real\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn block_scalar_with_explicit_indent_and_chomping() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "literal: |+2\n",
        "    &ignored anchor\n",
        "    content\n",
        "folded: |-\n",
        "  &alsoignored anchor\n",
        "after: value\n",
        "- &real anchor\n",
        "- *real\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn doc_boundary_inside_quotes_ignored() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!("---\n", "'---': &anchor value\n", "- *anchor\n",);
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn single_and_double_quote_handling() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "---\n",
        "- \"escaped \\\" quote\"\n",
        "- 'it''s fine'\n",
        "- &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn comment_stops_scanning() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "---\n",
        "- &anchor value # comment with *alias\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn blank_lines_are_ignored() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!("---\n", "\n", "\n", "- &anchor value\n", "- *anchor\n",);
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn block_scalar_allows_blank_lines_within_content() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "block: |\n",
        "  first\n",
        "\n",
        "  second\n",
        "after: value\n",
        "- &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn nested_block_scalar_handles_outdent() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "outer:\n",
        "  inner: |\n",
        "    line\n",
        "\n",
        "  next: value\n",
        "- &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn block_scalar_with_zero_indent_indicator() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "literal: |0\n",
        "text\n",
        "- &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn pipe_in_flow_is_not_block_indicator() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!("---\n", "- [|, &anchor value]\n", "- *anchor\n",);
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn alias_token_without_name_is_ignored() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!("---\n", "- *\n", "- &anchor value\n", "- *anchor\n",);
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn alias_at_document_start_is_reported() {
    let cfg = Config::new_for_tests(true, false, false);
    let yaml = concat!("---\n", "*missing\n");
    let hits = anchors::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![violation(
            2,
            1,
            &format!(r#"{MESSAGE_UNDECLARED_ALIAS} "missing""#)
        )]
    );
}

#[test]
fn block_indicator_with_unexpected_suffix_is_not_special() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "value: |x\n",
        "  text\n",
        "- &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn block_indicator_with_spaces_before_indent_value() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "value: |  2\n",
        "    text\n",
        "- &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn block_scalar_dedent_releases_state_immediately() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "block: |\n",
        "  inside\n",
        "outdent: &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn block_scalar_without_indented_content_releases_state() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!("block: |\n", "value\n", "- &anchor value\n", "- *anchor\n",);
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn doc_boundary_with_leading_whitespace() {
    let cfg = Config::new_for_tests(false, false, false);
    let yaml = concat!(
        "  ---\n",
        "- &anchor value\n",
        "  ...\n",
        "---\n",
        "- &other value\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn doc_boundary_with_trailing_whitespace_resets_state() {
    let cfg = Config::new_for_tests(false, false, false);
    let yaml = concat!(
        "---   \n",
        "- &anchor value\n",
        "...   \n",
        "---   \n",
        "- &other value\n",
        "- *other\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn doc_boundary_with_comment_is_detected() {
    let cfg = Config::new_for_tests(false, false, false);
    let yaml = concat!(
        "- &anchor value\n",
        "- *anchor\n",
        "--- # next document\n",
        "- &other value\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn partial_doc_marker_is_ignored() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!("--\n", "- &anchor value\n", "- *anchor\n",);
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn doc_boundary_detects_plain_markers() {
    let cfg = Config::new_for_tests(false, false, false);
    let yaml = concat!(
        "---\n",
        "- &anchor value\n",
        "...\n",
        "---\n",
        "- &other value\n",
        "- *other\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn doc_boundary_ignored_inside_multiline_single_quote() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "'value\n",
        "---\n",
        "line'\n",
        "- &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn doc_start_marker_mid_stream_resets_state() {
    let cfg = Config::new_for_tests(true, false, false);
    let yaml = concat!("key: value\n", "---\n", "- &anchor value\n", "- *anchor\n",);
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn block_inconsistent_indent_clears_state() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = concat!(
        "block: |\n",
        "    first\n",
        "   second\n",
        "- &anchor value\n",
        "- *anchor\n",
    );
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn ambiguous_names_reported_for_both_anchor_and_alias() {
    let yaml = "---\na: &foo: 42\nm:\n  - bar\n  - *foo:\n";
    let hits = anchors::check(yaml, &ambiguous_cfg());
    assert_eq!(
        hits,
        vec![
            violation(2, 4, &format!(r#"{MESSAGE_AMBIGUOUS_ANCHOR} "foo:""#)),
            violation(5, 5, &format!(r#"{MESSAGE_AMBIGUOUS_ALIAS} "foo:""#)),
        ]
    );
}

#[test]
fn ambiguous_internal_colon_reports_full_welded_name() {
    let yaml = "a: &foo:bar 42\n";
    let hits = anchors::check(yaml, &ambiguous_cfg());
    assert_eq!(
        hits,
        vec![violation(
            1,
            4,
            &format!(r#"{MESSAGE_AMBIGUOUS_ANCHOR} "foo:bar""#)
        )]
    );
}

#[test]
fn ambiguous_colon_leading_names_are_reported() {
    let anchor = anchors::check("a: &:foo 1\n", &ambiguous_cfg());
    assert_eq!(
        anchor,
        vec![violation(
            1,
            4,
            &format!(r#"{MESSAGE_AMBIGUOUS_ANCHOR} ":foo""#)
        )]
    );
    // The names resolve to `:` (spec/granit), so `*:` references the declared `&:`
    // (used, not undeclared); both carry the welded colon and are flagged ambiguous.
    let both = anchors::check("a: &: 1\nb: *:\n", &ambiguous_cfg());
    assert_eq!(
        both,
        vec![
            violation(1, 4, &format!(r#"{MESSAGE_AMBIGUOUS_ANCHOR} ":""#)),
            violation(2, 4, &format!(r#"{MESSAGE_AMBIGUOUS_ALIAS} ":""#)),
        ]
    );
}

#[test]
fn space_separated_alias_key_is_not_ambiguous() {
    let yaml = "---\na: &foo 42\nm:\n  *foo : bar\n";
    let hits = anchors::check(yaml, &ambiguous_cfg());
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}

#[test]
fn ampersand_inside_plain_scalar_is_not_an_anchor() {
    // `&` after a non-space (`rock&roll`) or after a plain-scalar word and a space
    // (`word &x:y`) is part of the plain scalar (granit/yamllint agree), not an
    // anchor: the granit scanner tokenises both as a single `Scalar`, so the rule
    // sees no anchor to flag or register.
    let cfg = Config::new_for_tests(true, false, true)
        .with_forbid_ambiguous_anchor_alias_names(true);
    for yaml in ["value: rock&roll:thing\n", "v: word &x:y\n"] {
        let hits = anchors::check(yaml, &cfg);
        assert!(
            hits.is_empty(),
            "unexpected diagnostics for {yaml:?}: {hits:?}"
        );
    }
}

#[test]
fn glued_indicator_is_not_a_node_but_glued_flow_opener_is() {
    let cfg = Config::new_for_tests(true, false, false)
        .with_forbid_ambiguous_anchor_alias_names(true);
    // A `*`/`&` glued to a `-` or `:` (no space) is plain-scalar text, not a node:
    // a glob path and a colon-less mapping value stay clean.
    for yaml in ["run: dist/airflow-*.tgz\n", "k: v&x:y\n"] {
        let hits = anchors::check(yaml, &cfg);
        assert!(
            hits.is_empty(),
            "unexpected diagnostics for {yaml:?}: {hits:?}"
        );
    }
    // A `*` glued to a flow opener `[` is a real alias, so it is still detected.
    let flow = anchors::check("k: [*a]\n", &cfg);
    assert_eq!(
        flow,
        vec![violation(
            1,
            5,
            &format!(r#"{MESSAGE_UNDECLARED_ALIAS} "a""#)
        )]
    );
}

#[test]
fn ambiguous_check_off_by_default() {
    let cfg = Config::new_for_tests(true, false, false);
    let yaml = "a: &foo: 42\n";
    let hits = anchors::check(yaml, &cfg);
    assert!(hits.is_empty(), "unexpected diagnostics: {hits:?}");
}
