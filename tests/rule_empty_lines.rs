use ryl::config::YamlLintConfig;
use ryl::rules::empty_lines::{self, Config};

fn resolve(contents: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(contents).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn default_allows_two_blank_lines() {
    let cfg = resolve("rules:\n  empty-lines: enable\n");
    let ok = "key: value\n\n\nnext: item\n";
    let violations = empty_lines::check(ok, &cfg);
    assert!(
        violations.is_empty(),
        "unexpected diagnostics: {violations:?}"
    );

    let bad = "key: value\n\n\n\nnext: item\n";
    let hits = empty_lines::check(bad, &cfg);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.line, 4);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, "too many blank lines (3 > 2)");
}

#[test]
fn exceeds_max_reports_violation() {
    let cfg = resolve("rules:\n  empty-lines:\n    max: 0\n    max-start: 0\n    max-end: 0\n");
    let input = "---\nvalue: 1\n\nother: 2\n";
    let hits = empty_lines::check(input, &cfg);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.line, 3);
    assert_eq!(hit.message, "too many blank lines (1 > 0)");
}

#[test]
fn start_limit_applied_before_general_max() {
    let cfg = resolve("rules:\n  empty-lines:\n    max: 5\n    max-start: 1\n    max-end: 0\n");
    let input = "\n\nkey: value\n";
    let hits = empty_lines::check(input, &cfg);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.line, 2);
    assert_eq!(hit.message, "too many blank lines (2 > 1)");
}

#[test]
fn end_limit_overrides_general_max() {
    let cfg = resolve("rules:\n  empty-lines:\n    max: 5\n    max-start: 0\n    max-end: 1\n");
    let input = "key: value\n\n\n";
    let hits = empty_lines::check(input, &cfg);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.line, 3);
    assert_eq!(hit.message, "too many blank lines (2 > 1)");
}

#[test]
fn single_newline_file_is_ignored() {
    let cfg = resolve("rules:\n  empty-lines:\n    max: 0\n    max-start: 0\n    max-end: 0\n");
    let hits = empty_lines::check("\n", &cfg);
    assert!(
        hits.is_empty(),
        "single newline should not produce violations"
    );

    let crlf_hits = empty_lines::check("\r\n", &cfg);
    assert!(
        crlf_hits.is_empty(),
        "single CRLF newline should not produce violations"
    );
}

#[test]
fn space_only_lines_are_not_blank() {
    let cfg = resolve("rules:\n  empty-lines:\n    max: 0\n    max-start: 0\n    max-end: 0\n");
    let input = "---\nintro\n \nend\n";
    let hits = empty_lines::check(input, &cfg);
    assert!(
        hits.is_empty(),
        "space-only lines should not be treated as blank"
    );
}
