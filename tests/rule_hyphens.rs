use ryl::rules::hyphens::{self, Config, Violation};

#[test]
fn allows_single_space_after_hyphen() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("- item\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_too_many_spaces_in_root_sequence() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("-  item\n", &cfg);
    assert_eq!(diagnostics, vec![Violation { line: 1, column: 3 }]);
}

#[test]
fn reports_too_many_spaces_with_indentation() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("  -  item\n", &cfg);
    assert_eq!(diagnostics, vec![Violation { line: 1, column: 5 }]);
}

#[test]
fn respects_configured_max_spaces() {
    let cfg = Config::new_for_tests(3);
    let diagnostics = hyphens::check("-    item\n", &cfg);
    assert_eq!(diagnostics, vec![Violation { line: 1, column: 5 }]);

    let ok = hyphens::check("-   item\n", &cfg);
    assert!(ok.is_empty(), "unexpected diagnostics: {ok:?}");
}

#[test]
fn ignores_entries_with_comments_only() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("-  # comment\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ignores_blank_lines() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("\n- item\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ignores_entries_without_inline_values() {
    let cfg = Config::new_for_tests(1);
    let diagnostics = hyphens::check("-\n  key: value\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}
