use ryl::rules::comments_indentation::{self, Config, Violation};

fn run(input: &str) -> Vec<Violation> {
    comments_indentation::check(input, &Config)
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
fn empty_block_scalar_resets_state() {
    let input = "value: |\nnext: item\n";
    let hits = run(input);
    assert!(
        hits.is_empty(),
        "empty block scalars should not cause diagnostics: {hits:?}"
    );
}
