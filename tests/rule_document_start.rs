use ryl::rules::document_start::{self, Config, FORBIDDEN_MESSAGE, MISSING_MESSAGE};

#[test]
fn reports_missing_marker_at_start_of_file() {
    let cfg = Config::new(true);
    let input = "foo: bar\n";
    let hits = document_start::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected a violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 1);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, MISSING_MESSAGE);
}

#[test]
fn reports_missing_marker_after_comment_block() {
    let cfg = Config::new(true);
    let input = "# header\nfoo: bar\n";
    let hits = document_start::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected a violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 2);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, MISSING_MESSAGE);
}

#[test]
fn explicit_marker_satisfies_requirement() {
    let cfg = Config::new(true);
    let input = "---\nfoo: bar\n";
    let hits = document_start::check(input, &cfg);
    assert!(hits.is_empty(), "explicit marker should pass: {hits:?}");
}

#[test]
fn forbidding_marker_flags_explicit_documents() {
    let cfg = Config::new(false);
    let input = "---\nfoo: bar\n";
    let hits = document_start::check(input, &cfg);
    assert_eq!(hits.len(), 1, "expected a violation: {hits:?}");
    let hit = &hits[0];
    assert_eq!(hit.line, 1);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, FORBIDDEN_MESSAGE);
}

#[test]
fn implicit_documents_respected_when_forbidden() {
    let cfg = Config::new(false);
    let input = "foo: bar\n";
    let hits = document_start::check(input, &cfg);
    assert!(hits.is_empty(), "implicit document start allowed: {hits:?}");
}

#[test]
fn empty_stream_has_no_diagnostics() {
    let cfg = Config::new(true);
    let hits = document_start::check("", &cfg);
    assert!(hits.is_empty(), "empty stream should not warn: {hits:?}");
}

#[test]
fn reports_missing_marker_at_a_root_block_scalar_header() {
    let cfg = Config::new(true);
    for input in [">\n  text\n", "|-\n\n  text\n", "a: 1\n...\n|\n  text\n"] {
        let hits = document_start::check(input, &cfg);
        let header = input
            .lines()
            .position(|line| line.starts_with(['>', '|']))
            .unwrap()
            + 1;
        assert_eq!(
            hits.last().map(|hit| hit.line),
            Some(header),
            "{input:?}: {hits:?}"
        );
    }
}

#[test]
fn forbidden_marker_after_an_empty_block_scalar_keeps_its_line() {
    let hits = document_start::check("|\n---\nvalue\n", &Config::new(false));
    assert_eq!(
        hits.iter()
            .map(|hit| (hit.line, hit.column, hit.message.as_str()))
            .collect::<Vec<_>>(),
        [(2, 1, FORBIDDEN_MESSAGE)]
    );
}
