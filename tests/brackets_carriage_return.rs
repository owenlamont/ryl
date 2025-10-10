use ryl::rules::brackets::{Config, Forbid, check};

fn config(forbid: Forbid) -> Config {
    Config::new_for_tests(forbid, 0, 0, -1, -1)
}

#[test]
fn brackets_carriage_return_without_line_feed_is_retained() {
    let violations = check("[\r]", &config(Forbid::None));
    assert!(violations.is_empty());
}

#[test]
fn brackets_comment_crlf_is_ignored() {
    let diagnostics = check("[# comment\r\n]", &config(Forbid::None));
    assert!(diagnostics.is_empty());
}

#[test]
fn brackets_comment_carriage_return_is_ignored() {
    let diagnostics = check("[# comment\r]", &config(Forbid::None));
    assert!(diagnostics.is_empty());
}

#[test]
fn brackets_carriage_return_line_feed_is_ignored() {
    let diagnostics = check("[\r\n]", &config(Forbid::None));
    assert!(diagnostics.is_empty());
}

#[test]
fn brackets_comment_newline_is_ignored() {
    let diagnostics = check("[# comment\n ]", &config(Forbid::None));
    assert!(diagnostics.is_empty());
}

#[test]
fn brackets_non_scalar_marks_sequence_non_empty() {
    let cfg = Config::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    let diagnostics = check("[ value ]", &cfg);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message == "forbidden flow sequence")
    );
}

#[test]
fn brackets_records_spacing_after_open() {
    let cfg = Config::new_for_tests(Forbid::None, 1, 1, -1, -1);
    let diagnostics = check("[value]", &cfg);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message == "too few spaces inside brackets")
    );
}

#[test]
fn brackets_records_spacing_too_many() {
    let cfg = Config::new_for_tests(Forbid::None, -1, 0, -1, -1);
    let diagnostics = check("[  value]", &cfg);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message == "too many spaces inside brackets")
    );
}

#[test]
fn brackets_spacing_ignored_across_newline() {
    let diagnostics = check("[\n]", &config(Forbid::None));
    assert!(diagnostics.is_empty());
}

#[test]
fn brackets_unmatched_closing_is_ignored() {
    let diagnostics = check("]", &config(Forbid::None));
    assert!(diagnostics.is_empty());
}

#[test]
fn brackets_plain_character_marks_sequence_non_empty() {
    let cfg = Config::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    let diagnostics = check("[a]", &cfg);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message == "forbidden flow sequence")
    );
}

#[test]
fn brackets_comment_without_newline_is_ignored() {
    let diagnostics = check("[# trailing comment", &config(Forbid::None));
    assert!(diagnostics.is_empty());
}

#[test]
fn brackets_truncated_sequence_skips_spacing_checks() {
    let diagnostics = check("[   ", &config(Forbid::None));
    assert!(diagnostics.is_empty());
}

#[test]
fn brackets_brace_content_marks_sequence_non_empty() {
    let cfg = Config::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    let diagnostics = check("[{key: 1}]", &cfg);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message == "forbidden flow sequence")
    );
}
