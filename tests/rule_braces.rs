use ryl::rules::braces::{self, Config, Forbid, Violation};

fn defaults() -> Config {
    Config::new_for_tests(Forbid::None, 0, 0, -1, -1)
}

#[test]
fn empty_input_returns_no_diagnostics() {
    let cfg = defaults();
    let diagnostics = braces::check("", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn accepts_compact_flow_mapping() {
    let cfg = defaults();
    let diagnostics = braces::check("object: {key: 1}\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_space_after_open_brace() {
    let cfg = defaults();
    let diagnostics = braces::check("object: { key: 1}\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 10,
            message: "too many spaces inside braces".to_string(),
        }]
    );
}

#[test]
fn reports_space_before_closing_brace() {
    let cfg = defaults();
    let diagnostics = braces::check("object: {key: 1 }\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 16,
            message: "too many spaces inside braces".to_string(),
        }]
    );
}

#[test]
fn forbid_true_rejects_flow_mapping() {
    let cfg = Config::new_for_tests(Forbid::All, 0, 0, -1, -1);
    let diagnostics = braces::check("object: {key: 1}\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 10,
            message: "forbidden flow mapping".to_string(),
        }]
    );
}

#[test]
fn forbid_non_empty_allows_empty_mapping() {
    let cfg = Config::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    let diagnostics = braces::check("object: {}\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn forbid_non_empty_rejects_non_empty_mapping() {
    let cfg = Config::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    let diagnostics = braces::check("object: {key: 1}\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 10,
            message: "forbidden flow mapping".to_string(),
        }]
    );
}

#[test]
fn min_spaces_inside_enforced_on_both_sides() {
    let cfg = Config::new_for_tests(Forbid::None, 1, -1, -1, -1);
    let diagnostics = braces::check("object: {key: 1}\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![
            Violation {
                line: 1,
                column: 10,
                message: "too few spaces inside braces".to_string(),
            },
            Violation {
                line: 1,
                column: 16,
                message: "too few spaces inside braces".to_string(),
            },
        ]
    );
}

#[test]
fn max_spaces_inside_limits_padding() {
    let cfg = Config::new_for_tests(Forbid::None, 0, 1, -1, -1);
    let diagnostics = braces::check("object: {  key: 1   }\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![
            Violation {
                line: 1,
                column: 11,
                message: "too many spaces inside braces".to_string(),
            },
            Violation {
                line: 1,
                column: 20,
                message: "too many spaces inside braces".to_string(),
            },
        ]
    );
}

#[test]
fn empty_mapping_spacing_overrides_defaults() {
    let cfg = Config::new_for_tests(Forbid::None, 0, 0, 1, 2);
    let diagnostics = braces::check("object: {}\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 10,
            message: "too few spaces inside empty braces".to_string(),
        }]
    );

    let diagnostics = braces::check("object: {    }\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 13,
            message: "too many spaces inside empty braces".to_string(),
        }]
    );
}

#[test]
fn multiline_mappings_skip_spacing_checks() {
    let cfg = defaults();
    let diagnostics = braces::check("mapping: {\n  key: value\n}\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn braces_inside_scalars_are_ignored() {
    let cfg = defaults();
    let diagnostics = braces::check("value: \"{ not a mapping }\"\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn comments_inside_flow_mappings_are_ignored() {
    let cfg = defaults();
    let diagnostics = braces::check("object: {key: value, # comment\n  other: 2}\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn carriage_returns_inside_flow_are_ignored() {
    let cfg = defaults();
    let diagnostics = braces::check("object: {key: 1,\r\n  other: 2}\r\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn nested_mappings_mark_outer_non_empty() {
    let cfg = defaults();
    let diagnostics = braces::check("outer: {{inner: 1}}\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn unmatched_closing_brace_is_ignored() {
    let cfg = defaults();
    let diagnostics = braces::check("}\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}
