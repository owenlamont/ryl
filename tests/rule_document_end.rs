use ryl::rules::document_end::{
    self, Config, FORBIDDEN_MESSAGE, MISSING_MESSAGE, classify_document_end_marker_bytes,
};

#[test]
fn reports_missing_marker_at_stream_end() {
    let cfg = Config::new_for_tests(true);
    let input = "---\nwithout:\n  document: end\n";
    let hits = document_end::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected a violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 3);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, MISSING_MESSAGE);
}

#[test]
fn reports_missing_marker_between_documents() {
    let cfg = Config::new_for_tests(true);
    let input = "---\nfirst: document\n---\nsecond: document\n";
    let hits = document_end::check(input, &cfg);
    assert_eq!(hits.len(), 2, "expected two violations: {hits:?}");
    assert_eq!(hits[0].line, 3);
    assert_eq!(hits[0].column, 1);
    assert_eq!(hits[0].message, MISSING_MESSAGE);
    assert_eq!(hits[1].line, 4);
    assert_eq!(hits[1].column, 1);
    assert_eq!(hits[1].message, MISSING_MESSAGE);
}

#[test]
fn explicit_marker_satisfies_requirement() {
    let cfg = Config::new_for_tests(true);
    let input = "---\nwith:\n  document: end\n...\n";
    let hits = document_end::check(input, &cfg);
    assert!(hits.is_empty(), "explicit marker should pass: {hits:?}");
}

#[test]
fn forbidding_marker_flags_explicit_marker() {
    let cfg = Config::new_for_tests(false);
    let input = "---\nwith:\n  document: end\n...\n";
    let hits = document_end::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected a violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 4);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, FORBIDDEN_MESSAGE);
}

#[test]
fn forbidding_marker_allows_absent_marker() {
    let cfg = Config::new_for_tests(false);
    let input = "---\nwith:\n  document: end\n";
    let hits = document_end::check(input, &cfg);
    assert!(
        hits.is_empty(),
        "missing marker allowed when forbidden: {hits:?}"
    );
}

#[test]
fn empty_stream_has_no_diagnostics() {
    let cfg = Config::new_for_tests(true);
    let hits = document_end::check("", &cfg);
    assert!(hits.is_empty(), "empty stream should not warn: {hits:?}");
}

#[test]
fn explicit_marker_with_trailing_spaces_is_still_detected() {
    let cfg = Config::new_for_tests(false);
    let input = "---\nwith:\n  document: end\n...   \n";
    let hits = document_end::check(input, &cfg);
    assert_eq!(
        hits.len(),
        1,
        "expected forbidden marker violation: {hits:?}"
    );
    let hit = &hits[0];
    assert_eq!(hit.line, 4);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, FORBIDDEN_MESSAGE);
}

#[test]
fn marker_with_leading_indent_is_allowed() {
    let cfg = Config::new_for_tests(true);
    let input = "---\nwith:\n  document: end\n  ...\n";
    let hits = document_end::check(input, &cfg);
    assert!(
        hits.is_empty(),
        "indented marker should satisfy requirement: {hits:?}"
    );
}

#[test]
fn marker_with_inline_comment_is_allowed() {
    let cfg = Config::new_for_tests(true);
    let input = "---\nwith:\n  document: end\n... # done\n";
    let hits = document_end::check(input, &cfg);
    assert!(
        hits.is_empty(),
        "marker followed by comment should satisfy requirement: {hits:?}"
    );
}

#[test]
fn classify_marker_bytes_trims_whitespace() {
    assert_eq!(classify_document_end_marker_bytes(b"  ...   "), Some("..."));
    assert_eq!(classify_document_end_marker_bytes(b"\t---\r"), Some("---"));
}

#[test]
fn classify_marker_bytes_rejects_comments_and_empty() {
    assert!(classify_document_end_marker_bytes(b"... # done").is_none());
    assert!(classify_document_end_marker_bytes(b"").is_none());
}
