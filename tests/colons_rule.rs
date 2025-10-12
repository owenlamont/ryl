use ryl::rules::colons::{self, Config};

fn violation_points(content: &str, cfg: Config) -> Vec<(usize, usize, String)> {
    let mut hits = colons::check(content, &cfg);
    hits.sort_by(|a, b| a.line.cmp(&b.line).then(a.column.cmp(&b.column)));
    hits.into_iter()
        .map(|hit| (hit.line, hit.column, hit.message))
        .collect()
}

#[test]
fn config_getters_return_values() {
    let cfg = Config::new_for_tests(3, 4);
    assert_eq!(cfg.max_spaces_before(), 3);
    assert_eq!(cfg.max_spaces_after(), 4);
}

#[test]
fn no_violation_with_defaults() {
    let cfg = Config::new_for_tests(0, 1);
    let points = violation_points("key: value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn empty_input_returns_no_violations() {
    let cfg = Config::new_for_tests(0, 1);
    let points = violation_points("", cfg);
    assert!(points.is_empty());
}

#[test]
fn detects_excess_spaces_before_colon() {
    let cfg = Config::new_for_tests(0, -1);
    let points = violation_points("key : value\n", cfg);
    assert_eq!(
        points,
        vec![(1, 4, "too many spaces before colon".to_string())]
    );
}

#[test]
fn detects_excess_spaces_after_colon() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("key:  value\n", cfg);
    assert_eq!(
        points,
        vec![(1, 6, "too many spaces after colon".to_string())]
    );
}

#[test]
fn detects_excess_spaces_after_question_mark() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("?  key\n: value\n", cfg);
    assert_eq!(
        points,
        vec![(1, 3, "too many spaces after question mark".to_string())],
    );
}

#[test]
fn ignores_alias_immediately_before_colon() {
    let cfg = Config::new_for_tests(0, 1);
    let points = violation_points("- anchor: &a key\n- *a: 42\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn flags_alias_with_extra_spaces() {
    let cfg = Config::new_for_tests(0, 1);
    let points = violation_points("- anchor: &a key\n- *a  : 42\n", cfg);
    assert_eq!(
        points,
        vec![(2, 6, "too many spaces before colon".to_string())]
    );
}

#[test]
fn skips_colons_inside_comments() {
    let cfg = Config::new_for_tests(0, 1);
    let points = violation_points("# comment: text\nkey: value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn handles_crlf_after_colon() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("key:\r\n  value\rnext: pair\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn question_mark_not_explicit_is_ignored() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("value? trailing\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn sequence_question_mark_spacing_detected() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("- ?  key\n  :  value\n", cfg);
    assert_eq!(
        points,
        vec![
            (1, 5, "too many spaces after question mark".to_string()),
            (2, 5, "too many spaces after colon".to_string()),
        ]
    );
}

#[test]
fn question_mark_spacing_disabled_skips_check() {
    let cfg = Config::new_for_tests(-1, -1);
    let points = violation_points("?  key\n: value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn comment_with_crlf_is_ignored() {
    let cfg = Config::new_for_tests(0, 1);
    let points = violation_points("# note: here\r\nkey: value\r\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn colon_at_line_start_reports_after_spacing() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points(":  value\n", cfg);
    assert_eq!(
        points,
        vec![(1, 3, "too many spaces after colon".to_string())]
    );
}

#[test]
fn colon_at_end_of_file_is_ignored() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("key:", cfg);
    assert!(points.is_empty());
}

#[test]
fn colon_followed_by_carriage_return_is_ignored() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("key:\rvalue\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn flow_question_mark_spacing_detected() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("[?  key: value]\n", cfg);
    assert_eq!(
        points,
        vec![(1, 4, "too many spaces after question mark".to_string())]
    );
}

#[test]
fn indented_sequence_question_mark_spacing_detected() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("parent:\n  - ?  child\n    :  value\n", cfg);
    assert_eq!(
        points,
        vec![
            (2, 7, "too many spaces after question mark".to_string()),
            (3, 7, "too many spaces after colon".to_string()),
        ]
    );
}

#[test]
fn hyphen_not_sequence_does_not_trigger_question_mark_rule() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("a- ? key\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn question_mark_without_space_not_explicit() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("?key: value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn dash_without_space_before_question_mark_is_ignored() {
    let cfg = Config::new_for_tests(-1, 1);
    let points = violation_points("-?  key\n  : value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn colons_inside_scalars_with_multibyte_chars_are_ignored() {
    let cfg = Config::new_for_tests(0, 1);
    let points = violation_points("key: \"café: menu\"\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn inline_comment_after_colon_is_ignored_for_spacing() {
    let cfg = Config::new_for_tests(-1, 0);
    let points = violation_points("key:  # note\n", cfg);
    assert!(
        points.is_empty(),
        "inline comment should bypass spacing check: {points:?}"
    );
}

#[test]
fn coverage_explicit_question_mark_handles_non_whitespace_next() {
    let chars: Vec<(usize, char)> = "?key".char_indices().collect();
    assert!(!colons::coverage_is_explicit_question_mark(&chars, 0));
}

#[test]
fn coverage_explicit_question_mark_handles_sequence_indicator_branch() {
    let chars: Vec<(usize, char)> = " - ? key".char_indices().collect();
    assert!(colons::coverage_is_explicit_question_mark(&chars, 3));
}

#[test]
fn coverage_sequence_indicator_false_branch() {
    let chars: Vec<(usize, char)> = "a-".char_indices().collect();
    assert!(!colons::coverage_is_sequence_indicator(&chars, 1));
}

#[test]
fn coverage_explicit_question_mark_handles_regular_char_before() {
    let chars: Vec<(usize, char)> = "a ? ".char_indices().collect();
    assert!(!colons::coverage_is_explicit_question_mark(&chars, 2));
}

#[test]
fn coverage_evaluate_question_mark_reports_violation() {
    let cfg = Config::new_for_tests(-1, 1);
    let violations = colons::coverage_evaluate_question_mark("?  key\n: value\n", &cfg);
    assert!(
        violations
            .iter()
            .any(|v| v.message.contains("too many spaces after question mark"))
    );
}

#[test]
fn coverage_explicit_question_mark_handles_flow_prefix_variants() {
    let bracket_chars: Vec<(usize, char)> = "[ ? key".char_indices().collect();
    assert!(colons::coverage_is_explicit_question_mark(
        &bracket_chars,
        2
    ));
    let brace_chars: Vec<(usize, char)> = "{ ? key".char_indices().collect();
    assert!(colons::coverage_is_explicit_question_mark(&brace_chars, 2));
    let comma_chars: Vec<(usize, char)> = ", ? key".char_indices().collect();
    assert!(colons::coverage_is_explicit_question_mark(&comma_chars, 2));
}

#[test]
fn coverage_question_mark_immediate_newline_is_ignored() {
    let cfg = Config::new_for_tests(-1, 1);
    let violations = colons::coverage_evaluate_question_mark("? \n: value\n", &cfg);
    assert!(violations.is_empty());
}

#[test]
fn coverage_question_mark_crlf_is_ignored() {
    let cfg = Config::new_for_tests(-1, 1);
    let violations = colons::coverage_evaluate_question_mark("? \r\n: value\n", &cfg);
    assert!(violations.is_empty());
}

#[test]
fn coverage_skip_comment_handles_crlf() {
    assert!(colons::coverage_skip_comment("# comment\r\nrest"));
}

#[test]
fn coverage_skip_comment_handles_standalone_cr() {
    assert!(!colons::coverage_skip_comment("# comment\rrest"));
}

#[test]
fn coverage_evaluate_question_mark_without_marker_is_noop() {
    let cfg = Config::new_for_tests(-1, 1);
    let violations = colons::coverage_evaluate_question_mark("key: value\n", &cfg);
    assert!(violations.is_empty());
}

#[test]
fn coverage_question_mark_within_limit_on_same_line() {
    let cfg = Config::new_for_tests(-1, 1);
    let violations = colons::coverage_evaluate_question_mark("? key: value\n", &cfg);
    assert!(violations.is_empty());
}

#[test]
fn coverage_check_handles_comment_crlf() {
    let cfg = Config::new_for_tests(0, 1);
    let result = colons::check("# heading\r\nkey: value\n", &cfg);
    assert!(result.is_empty());
}
