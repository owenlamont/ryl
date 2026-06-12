use ryl::rules::comments_indentation::{self, Config, Violation};

fn run(input: &str) -> Vec<Violation> {
    comments_indentation::check(input, &Config::default())
}

fn run_open(input: &str) -> Vec<Violation> {
    comments_indentation::check(input, &Config::new_for_tests(true))
}

#[test]
fn empty_input_returns_no_hits() {
    let hits = run("");
    assert!(hits.is_empty());
}

#[test]
fn accepts_aligned_comment_inside_mapping() {
    let input = "obj:\n  # ok\n  value: 1\n";
    let hits = run(input);
    assert!(hits.is_empty());
}

#[test]
fn rejects_comment_with_extra_indent() {
    let input = "obj:\n # wrong\n  value: 1\n";
    let hits = run(input);
    assert_eq!(hits, vec![Violation { line: 2, column: 2 }]);
}

#[test]
fn rejects_comment_after_comment_block_reset() {
    let input = "obj1:\n  a: 1\n# heading\n  # misplaced\nobj2: no\n";
    let hits = run(input);
    assert_eq!(hits, vec![Violation { line: 4, column: 3 }]);
}

#[test]
fn rejects_comment_after_inline_comment() {
    let input = "- a  # inline\n # wrong\n";
    let hits = run(input);
    assert_eq!(hits, vec![Violation { line: 2, column: 2 }]);
}

#[test]
fn blank_line_keeps_comment_alignment() {
    let input = "# top\n\n  # wrong\nvalue: 1\n";
    let hits = run(input);
    assert_eq!(hits, vec![Violation { line: 3, column: 3 }]);
}

#[test]
fn allows_comment_inside_block_scalar_body() {
    let input = "rule:\n  - pattern: |\n      body\n    # example\n  - other: value\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "block scalar comment should be ignored: {hits:?}"
    );
}

#[test]
fn allows_comment_dedented_to_indicator_indent() {
    let input = "rule:\n  - pattern: |\n      body\n  # metadata\n  - other: value\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "comment aligned with indicator should pass: {hits:?}"
    );
}

#[test]
fn indicator_with_trailing_comment_is_supported() {
    let input = "job:\n  run: | # trailing comment\n    step one\n  next: value\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "inline comment on indicator should be ignored: {hits:?}"
    );
}

#[test]
fn block_scalar_allows_blank_line() {
    let input = "rule:\n  - pattern: |\n      alpha\n\n      omega\n  - other: value\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "blank lines inside block scalars should pass: {hits:?}"
    );
}

#[test]
fn inline_quotes_and_escapes_before_comment_are_handled() {
    let with_single = "value: 'quoted # fragment' # note\n  # aligned\n";
    let hits = run(with_single);
    assert_eq!(hits, vec![Violation { line: 2, column: 3 }]);

    let with_escape = "path: \"dir\\#name\" # note\n  # aligned\n";
    let hits = run(with_escape);
    assert_eq!(hits, vec![Violation { line: 2, column: 3 }]);
}

#[test]
fn block_scalar_followed_by_mapping_is_handled() {
    let input = "value: |\n  text\nnext: 1\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "block scalar should reset tracker before next mapping: {hits:?}"
    );
}

#[test]
fn folded_block_scalar_with_chomping_is_detected() {
    let input = "rule:\n  value: >-\n    body\n  # metadata\n  next: value\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "folded block scalar with chomping should not flag comments: {hits:?}"
    );
}

#[test]
fn empty_block_scalar_resets_state() {
    let input = "value: |\nnext: item\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "empty block scalars should not cause diagnostics: {hits:?}"
    );
}

#[test]
fn fix_aligns_misindented_comment() {
    let input = "obj:\n # wrong\n  value: 1\n";
    let fixed = comments_indentation::fix(input, &Config::default());
    assert_eq!(fixed, Some("obj:\n  # wrong\n  value: 1\n".to_string()));
}

#[test]
fn fix_aligns_comment_block_to_content_indent() {
    let input = "obj1:\n  a: 1\n# heading\n  # misplaced\nobj2: no\n";
    let fixed = comments_indentation::fix(input, &Config::default());
    assert_eq!(
        fixed,
        Some("obj1:\n  a: 1\n# heading\n# misplaced\nobj2: no\n".to_string())
    );
}

#[test]
fn fix_ignores_block_scalar_regions() {
    let input = "rule:\n  - pattern: |\n      body\n    # example\n  - other: value\n";
    let fixed = comments_indentation::fix(input, &Config::default());
    assert_eq!(fixed, None);
}

#[test]
fn fix_returns_none_when_already_aligned() {
    let input = "obj:\n  # ok\n  value: 1\n";
    let fixed = comments_indentation::fix(input, &Config::default());
    assert_eq!(fixed, None);
}

#[test]
fn fix_returns_none_for_empty_input() {
    let fixed = comments_indentation::fix("", &Config::default());
    assert_eq!(fixed, None);
}

#[test]
fn fix_preserves_comment_alignment_state_across_crlf_blank_lines() {
    let input = "root:\r\n  # first\r\n\r\n # second\r\n  value: 1\r\n";
    let fixed = comments_indentation::fix(input, &Config::default());
    assert_eq!(
        fixed,
        Some("root:\r\n  # first\r\n\r\n  # second\r\n  value: 1\r\n".to_string())
    );
}

#[test]
fn recognises_tagged_block_scalar_header() {
    let input = "key: !!str |\n  body\n  # inside-body\nnext: value\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "comments inside tagged block scalar should be skipped: {hits:?}"
    );
}

#[test]
fn recognises_anchored_block_scalar_header() {
    let input = "key: &anchor >\n  body\n  # inside-body\nnext: value\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "comments inside anchored block scalar should be skipped: {hits:?}"
    );
}

#[test]
fn recognises_top_level_anchor_then_block_scalar_marker() {
    let input = "&anchor |\n  body\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "top-level anchored block scalar should parse: {hits:?}"
    );
}

#[test]
fn rejects_block_marker_following_non_indicator_token() {
    let input = "key: value |\n  more\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "`|` after a plain scalar is not a block-scalar header: {hits:?}"
    );
}

#[test]
fn fix_resets_comment_block_at_directive_comment() {
    // A `# yamllint ` directive breaks the comment block (resetting the reference), so
    // the trailing misindented comment is re-indented to the surrounding content.
    let input = "obj:\n  a: 1\n# yamllint disable\n # misindented\nobj2: no\n";
    let fixed = comments_indentation::fix(input, &Config::default());
    assert_eq!(
        fixed,
        Some(
            "obj:\n  a: 1\n# yamllint disable\n  # misindented\nobj2: no\n".to_string()
        )
    );
}

#[test]
fn allow_any_open_indent_accepts_comment_at_open_block_level() {
    // The comment aligns with the open `items:` mapping level (0), not the following
    // sequence content (2): flagged by default, accepted with the option (#259).
    let input = "items:\n  - one\n# boundary\n  - two\n";
    assert_eq!(run(input), vec![Violation { line: 3, column: 1 }]);
    assert!(run_open(input).is_empty(), "open level should be accepted");
}

#[test]
fn allow_any_open_indent_accepts_middle_open_level() {
    // The comment matches the middle open level (2 = `b:`), not the innermost (4) or
    // outermost (0); since the following content `e:` is at 0, the default flags it.
    // Pins that `compute_open_indents` keeps interior block levels open (the middle
    // `b:` level survives until its mapping ends), not just the innermost/outermost.
    let input = "a:\n  b:\n    c: 1\n  # mid\ne: 2\n";
    assert_eq!(run(input), vec![Violation { line: 4, column: 3 }]);
    assert!(
        run_open(input).is_empty(),
        "middle open level should be accepted"
    );
}

#[test]
fn allow_any_open_indent_only_accepts_still_open_levels() {
    // Indent 4 was used by `deep:` but that level closed at `c: 2`; the option accepts
    // only *currently-open* levels (here {0, 2}), so the stale level 4 is still flagged.
    let input = "a:\n  b:\n    deep: 1\n  c: 2\n    # stale level\ne: 3\n";
    assert_eq!(run_open(input), vec![Violation { line: 5, column: 5 }]);
}

#[test]
fn allow_any_open_indent_accepts_compact_mapping_level() {
    // A list-of-mappings: `- key:` opens a mapping at col 4 (after `- `), which is never
    // a line's leading indent, so the open-level stack must record it from the compact
    // entry. Default flags the comment; the option accepts it (regression for the Codex
    // P2 finding on PR #289).
    let input = "items:\n  - key:\n      nested: x\n    # boundary\n  - next\n";
    assert_eq!(run(input), vec![Violation { line: 4, column: 5 }]);
    assert!(
        run_open(input).is_empty(),
        "comment at the compact `- key:` mapping level (col 4) should be accepted"
    );
}

#[test]
fn allow_any_open_indent_accepts_compact_nested_sequence_level() {
    // `- - x` opens an inner sequence at col 4 (the second dash); that level appears
    // only via the compact entry, never as a leading indent. The option accepts a
    // comment aligned to it.
    let input = "top:\n  - - x\n    # inner\n  - y\n";
    assert_eq!(run(input), vec![Violation { line: 3, column: 5 }]);
    assert!(
        run_open(input).is_empty(),
        "inner `- -` sequence level (col 4) should be accepted"
    );
}

#[test]
fn allow_any_open_indent_rejects_scalar_sequence_entry_column() {
    // Soundness boundary: a *scalar* entry `- value` opens no block at col 4, so the
    // option must NOT accept a comment there (only mapping/sequence entries open a
    // deeper level). Distinguishing this is why the level must be derived structurally.
    let input = "items:\n  - value\n    # stray\n  - other\n";
    assert_eq!(run_open(input), vec![Violation { line: 3, column: 5 }]);
}

#[test]
fn allow_any_open_indent_rejects_multiline_scalar_continuation_column() {
    // `key: text` then `  continuation` folds into one plain scalar; col 2 is the
    // scalar's continuation, NOT an open block. The option must not accept a comment
    // there (adversarial-Codex finding #289 — only the parser can tell this apart).
    let input = "key: text\n  continuation\n# boundary\n  # stray\nnext: x\n";
    assert_eq!(run_open(input), vec![Violation { line: 4, column: 3 }]);
}

#[test]
fn allow_any_open_indent_on_unparsable_input_degrades_to_base_rule() {
    // Open levels come from the parser; on a parse error there are none, so the option
    // behaves like the default rule (and never panics). `[` opens an unterminated flow.
    let input = "a: [\n  # x\nb: 2\n";
    let strict = run(input);
    let relaxed = run_open(input);
    assert_eq!(
        relaxed, strict,
        "with no parse, allow-any-open-indent must match the default rule"
    );
}

#[test]
fn allow_any_open_indent_ignores_flow_collection_columns() {
    // A flow mapping `{...}` carries no block indentation level, so its column is not
    // an open level (exercises the block-vs-flow check).
    let input = "items: {a: 1}\n   # x\nnext: 2\n";
    assert_eq!(run_open(input), vec![Violation { line: 2, column: 4 }]);
}

#[test]
fn allow_any_open_indent_ignores_implicit_flow_mapping_columns() {
    // The implicit mappings inside `[a: 1, b: 2]` start at a key (no `{`) but are flow
    // children, so their columns are not open levels: a comment aligned to one (col 8,
    // where `a` sits) is still flagged (adversarial-Codex finding #289).
    let input = "items: [a: 1, b: 2]\n        # x\nnext: 3\n";
    assert_eq!(run_open(input), vec![Violation { line: 2, column: 9 }]);
}

#[test]
fn allow_any_open_indent_rejects_url_scalar_entry_column() {
    // A `:` only indicates a mapping when followed by space/EOL, so `- http://example`
    // is a scalar (not a `http:` mapping). Its column must NOT be an open level, else
    // the option wrongly accepts a stray comment there (adversarial-Codex finding #289).
    let input = "items:\n  - http://example\n    # stray\n  - next\n";
    assert_eq!(run_open(input), vec![Violation { line: 3, column: 5 }]);
}

#[test]
fn allow_any_open_indent_accepts_single_quoted_backslash_key() {
    // `\` is literal in a single-quoted scalar, so `'a\'` closes and `'a\': value` is a
    // mapping opening at col 4. The classifier must not treat `\'` as an escaped quote
    // (adversarial-Codex finding #289).
    let input = "items:\n  - 'a\\': value\n    # boundary\n  - next\n";
    assert_eq!(run(input), vec![Violation { line: 3, column: 5 }]);
    assert!(
        run_open(input).is_empty(),
        "single-quoted-key mapping level (col 4) should be accepted"
    );
}

#[test]
fn allow_any_open_indent_accepts_compact_explicit_key() {
    // An explicit-key entry `- ? key` opens a mapping at col 4 just like `- key:`, so a
    // comment aligned there is accepted (adversarial-Codex finding #289).
    let input = "items:\n  - ? key\n    # boundary\n  - next\n";
    assert_eq!(run(input), vec![Violation { line: 3, column: 5 }]);
    assert!(
        run_open(input).is_empty(),
        "comment at the explicit-key mapping level (col 4) should be accepted"
    );
}

#[test]
fn allow_any_open_indent_fix_leaves_open_level_comment() {
    let input = "items:\n  - one\n# boundary\n  - two\n";
    assert_eq!(
        comments_indentation::fix(input, &Config::new_for_tests(true)),
        None
    );
}

#[test]
fn allow_any_open_indent_fix_reindents_genuine_violation() {
    // A comment matching no open level is still re-indented to the reference indent.
    let input = "a:\n  b:\n    deep: 1\n  c: 2\n    # stale level\ne: 3\n";
    assert_eq!(
        comments_indentation::fix(input, &Config::new_for_tests(true)),
        Some("a:\n  b:\n    deep: 1\n  c: 2\n  # stale level\ne: 3\n".to_string())
    );
}
