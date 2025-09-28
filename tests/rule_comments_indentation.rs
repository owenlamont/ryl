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
