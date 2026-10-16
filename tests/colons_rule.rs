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
    let cfg = Config::new(3, 4);
    assert_eq!(cfg.max_spaces_before(), 3);
    assert_eq!(cfg.max_spaces_after(), 4);
}

#[test]
fn no_violation_with_defaults() {
    let cfg = Config::new(0, 1);
    let points = violation_points("key: value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn empty_input_returns_no_violations() {
    let cfg = Config::new(0, 1);
    let points = violation_points("", cfg);
    assert!(points.is_empty());
}

#[test]
fn ignores_colon_inside_multibyte_quoted_scalar() {
    let cfg = Config::new(0, 1);
    let input = format!("key: \"{}:  x\"\n", "—".repeat(8));
    let points = violation_points(&input, cfg);
    assert!(
        points.is_empty(),
        "a colon inside a multibyte scalar must be ignored: {points:?}"
    );
}

#[test]
fn detects_excess_spaces_before_colon() {
    let cfg = Config::new(0, -1);
    let points = violation_points("key : value\n", cfg);
    assert_eq!(
        points,
        vec![(1, 4, "too many spaces before colon".to_string())]
    );
}

#[test]
fn detects_excess_spaces_after_colon() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("key:  value\n", cfg);
    assert_eq!(
        points,
        vec![(1, 6, "too many spaces after colon".to_string())]
    );
}

#[test]
fn detects_excess_spaces_after_question_mark() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("?  key\n: value\n", cfg);
    assert_eq!(
        points,
        vec![(1, 3, "too many spaces after question mark".to_string())],
    );
}

#[test]
fn exempts_required_space_before_alias_key_colon() {
    // `*a :` needs the space: without it `:` joins the alias name (`a:`), so the single
    // required space must not be flagged (adrienverge/yamllint#226).
    let cfg = Config::new(0, 1);
    let points = violation_points("- anchor: &a key\n- *a : 42\n", cfg);
    assert!(
        points.is_empty(),
        "required space before an alias-key colon must be allowed: {points:?}"
    );
}

#[test]
fn flags_extra_space_before_alias_key_colon() {
    let cfg = Config::new(0, 1);
    let points = violation_points("- anchor: &a key\n- *a  : 42\n", cfg);
    assert_eq!(
        points,
        vec![(2, 6, "too many spaces before colon".to_string())]
    );
}

#[test]
fn still_flags_spaces_after_alias_key_colon() {
    // The exemption covers only the required space *before* the colon; spacing after it
    // is still checked.
    let cfg = Config::new(0, 1);
    let points = violation_points("- anchor: &a key\n- *a :  42\n", cfg);
    assert_eq!(
        points,
        vec![(2, 8, "too many spaces after colon".to_string())]
    );
}

#[test]
fn flags_plain_scalar_key_containing_alias_marker() {
    // `foo *bar` is a plain scalar key, not an alias node, so the space before `:` is
    // still flagged: the exemption only applies where the parser resolved an alias.
    let cfg = Config::new(0, 1);
    let points = violation_points("foo *bar : baz\n", cfg);
    assert_eq!(
        points,
        vec![(1, 9, "too many spaces before colon".to_string())]
    );
}

#[test]
fn allows_document_initial_colon_with_null_key() {
    // Colon at the buffer start (a null mapping key): no key text precedes it, so there
    // are no excess spaces before the colon to report.
    let cfg = Config::new(0, 1);
    let points = violation_points(": value\n", cfg);
    assert!(
        points.is_empty(),
        "a document-initial null-key colon has no spacing violation: {points:?}"
    );
}

#[test]
fn exempts_required_space_for_undefined_alias_key() {
    // The exemption reads scanner tokens, so it fires for an alias key even when the
    // anchor is undefined or forward-referenced (the parser errors, but the token is
    // still an alias), matching yamllint, which exempts regardless of resolution.
    let cfg = Config::new(0, 1);
    let points = violation_points("*missing : 42\n", cfg);
    assert!(
        points.is_empty(),
        "an alias key is exempt even when its anchor is undefined: {points:?}"
    );
}

#[test]
fn skips_colons_inside_comments() {
    let cfg = Config::new(0, 1);
    let points = violation_points("# comment: text\nkey: value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn handles_crlf_after_colon() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("key:\r\n  value\rnext: pair\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn question_mark_not_explicit_is_ignored() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("value? trailing\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn sequence_question_mark_spacing_detected() {
    let cfg = Config::new(-1, 1);
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
    let cfg = Config::new(-1, -1);
    let points = violation_points("?  key\n: value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn comment_with_crlf_is_ignored() {
    let cfg = Config::new(0, 1);
    let points = violation_points("# note: here\r\nkey: value\r\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn colon_at_line_start_reports_after_spacing() {
    let cfg = Config::new(-1, 1);
    let points = violation_points(":  value\n", cfg);
    assert_eq!(
        points,
        vec![(1, 3, "too many spaces after colon".to_string())]
    );
}

#[test]
fn colon_at_end_of_file_is_ignored() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("key:", cfg);
    assert!(points.is_empty());
}

#[test]
fn colon_followed_by_carriage_return_is_ignored() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("key:\rvalue\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn flow_question_mark_spacing_detected() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("[?  key: value]\n", cfg);
    assert_eq!(
        points,
        vec![(1, 4, "too many spaces after question mark".to_string())]
    );
}

#[test]
fn indented_sequence_question_mark_spacing_detected() {
    let cfg = Config::new(-1, 1);
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
    let cfg = Config::new(-1, 1);
    let points = violation_points("a- ? key\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn question_mark_without_space_not_explicit() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("?key: value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn dash_without_space_before_question_mark_is_ignored() {
    let cfg = Config::new(-1, 1);
    let points = violation_points("-?  key\n  : value\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn colons_inside_scalars_with_multibyte_chars_are_ignored() {
    let cfg = Config::new(0, 1);
    let points = violation_points("key: \"café: menu\"\n", cfg);
    assert!(points.is_empty());
}

#[test]
fn inline_comment_after_colon_is_ignored_for_spacing() {
    let cfg = Config::new(-1, 0);
    let points = violation_points("key:  # note\n", cfg);
    assert!(
        points.is_empty(),
        "inline comment should bypass spacing check: {points:?}"
    );
}

#[test]
fn coverage_check_handles_comment_crlf() {
    let cfg = Config::new(0, 1);
    let result = colons::check("# heading\r\nkey: value\n", &cfg);
    assert!(result.is_empty());
}

#[test]
fn columns_count_characters_not_bytes_with_multibyte_key() {
    let cfg = Config::new(0, 1);
    let points = violation_points("ééé :  1\n", cfg);
    assert_eq!(
        points,
        vec![
            (1, 4, "too many spaces before colon".to_string()),
            (1, 7, "too many spaces after colon".to_string()),
        ]
    );
}

#[test]
fn exempts_required_space_before_anchor_or_tag_key_colon() {
    // `&an: v` is the scalar `v` anchored `an:`, so an empty anchored or tagged key keeps
    // one space before its `:`, like an alias key; a second space is still flagged.
    let cfg = Config::new(0, 1);
    assert_eq!(violation_points("&an : v\n!t : w\n", cfg), []);
    assert_eq!(
        violation_points("&an  : v\n", cfg),
        vec![(1, 5, "too many spaces before colon".to_string())]
    );
}
