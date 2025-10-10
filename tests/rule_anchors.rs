use ryl::rules::anchors::{
    self, Config, MESSAGE_DUPLICATED_ANCHOR, MESSAGE_UNDECLARED_ALIAS, MESSAGE_UNUSED_ANCHOR,
    Violation,
};

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
fn invalid_anchor_token_is_ignored() {
    let cfg = Config::new_for_tests(true, true, true);
    let yaml = "---\n- & value\n- *missing\n";
    let hits = anchors::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![violation(
            3,
            3,
            &format!(r#"{MESSAGE_UNDECLARED_ALIAS} "missing""#)
        )]
    );
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
