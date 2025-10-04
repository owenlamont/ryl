use ryl::rules::brackets::{self, Config, Forbid, Violation};

fn defaults() -> Config {
    Config::new_for_tests(Forbid::None, 0, 0, -1, -1)
}

#[test]
fn empty_input_returns_no_diagnostics() {
    let cfg = defaults();
    let diagnostics = brackets::check("", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn accepts_compact_flow_sequence() {
    let cfg = defaults();
    let diagnostics = brackets::check("object: [1, 2]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_space_after_open_bracket() {
    let cfg = defaults();
    let diagnostics = brackets::check("object: [ 1, 2]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 10,
            message: "too many spaces inside brackets".to_string(),
        }]
    );
}

#[test]
fn reports_space_before_closing_bracket() {
    let cfg = defaults();
    let diagnostics = brackets::check("object: [1, 2 ]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 14,
            message: "too many spaces inside brackets".to_string(),
        }]
    );
}

#[test]
fn forbid_true_rejects_flow_sequence() {
    let cfg = Config::new_for_tests(Forbid::All, 0, 0, -1, -1);
    let diagnostics = brackets::check("object: [1, 2]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 10,
            message: "forbidden flow sequence".to_string(),
        }]
    );
}

#[test]
fn forbid_non_empty_allows_empty_sequence() {
    let cfg = Config::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    let diagnostics = brackets::check("object: []\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn forbid_non_empty_rejects_non_empty_sequence() {
    let cfg = Config::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    let diagnostics = brackets::check("object: [1]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 10,
            message: "forbidden flow sequence".to_string(),
        }]
    );
}

#[test]
fn min_spaces_inside_enforced_on_both_sides() {
    let cfg = Config::new_for_tests(Forbid::None, 1, -1, -1, -1);
    let diagnostics = brackets::check("object: [1, 2]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![
            Violation {
                line: 1,
                column: 10,
                message: "too few spaces inside brackets".to_string(),
            },
            Violation {
                line: 1,
                column: 14,
                message: "too few spaces inside brackets".to_string(),
            },
        ]
    );
}

#[test]
fn max_spaces_inside_limits_padding() {
    let cfg = Config::new_for_tests(Forbid::None, 0, 1, -1, -1);
    let diagnostics = brackets::check("object: [  1, 2   ]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![
            Violation {
                line: 1,
                column: 11,
                message: "too many spaces inside brackets".to_string(),
            },
            Violation {
                line: 1,
                column: 18,
                message: "too many spaces inside brackets".to_string(),
            },
        ]
    );
}

#[test]
fn empty_sequence_spacing_overrides_defaults() {
    let cfg = Config::new_for_tests(Forbid::None, 0, 0, 1, 2);
    let diagnostics = brackets::check("object: []\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 10,
            message: "too few spaces inside empty brackets".to_string(),
        }]
    );

    let diagnostics = brackets::check("object: [    ]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 13,
            message: "too many spaces inside empty brackets".to_string(),
        }]
    );
}

#[test]
fn multiline_sequences_skip_spacing_checks() {
    let cfg = defaults();
    let diagnostics = brackets::check("seq: [\n  1,\n  2\n]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn brackets_inside_scalars_are_ignored() {
    let cfg = defaults();
    let diagnostics = brackets::check("value: \"[ not a sequence ]\"\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn comments_inside_flow_sequences_are_ignored() {
    let cfg = defaults();
    let diagnostics = brackets::check("object: [1, # comment\n  2]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn carriage_returns_inside_flow_are_ignored() {
    let cfg = defaults();
    let diagnostics = brackets::check("object: [1,\r\n  2]\r\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn nested_sequences_mark_outer_non_empty() {
    let cfg = defaults();
    let diagnostics = brackets::check("outer: [[1]]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn unmatched_closing_bracket_is_ignored() {
    let cfg = defaults();
    let diagnostics = brackets::check("]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}
