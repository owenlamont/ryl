use ryl::rules::commas::{self, Config, Violation};

fn defaults() -> Config {
    Config::new_for_tests(0, 1, 1)
}

#[test]
fn empty_input_returns_no_diagnostics() {
    let cfg = defaults();
    let diagnostics = commas::check("", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn accepts_well_spaced_flow_sequence() {
    let cfg = defaults();
    let diagnostics = commas::check("[1, 2]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn reports_space_before_comma() {
    let cfg = defaults();
    let diagnostics = commas::check("[1 ,2]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![
            Violation {
                line: 1,
                column: 3,
                message: "too many spaces before comma".to_string(),
            },
            Violation {
                line: 1,
                column: 5,
                message: "too few spaces after comma".to_string(),
            },
        ]
    );
}

#[test]
fn reports_missing_space_after_comma() {
    let cfg = defaults();
    let diagnostics = commas::check("[1,2]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 4,
            message: "too few spaces after comma".to_string(),
        }]
    );
}

#[test]
fn reports_excess_space_after_comma() {
    let cfg = defaults();
    let diagnostics = commas::check("[1,  2]\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![Violation {
            line: 1,
            column: 5,
            message: "too many spaces after comma".to_string(),
        }]
    );
}

#[test]
fn ignores_newline_after_comma() {
    let cfg = defaults();
    let diagnostics = commas::check("[1,\n  2]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ignores_comma_at_line_start() {
    let cfg = defaults();
    let diagnostics = commas::check("[\n  1\n, 2]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn newline_before_comma_is_ignored() {
    let cfg = defaults();
    let diagnostics = commas::check("[1\n, 2]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn ignores_commas_inside_scalars() {
    let cfg = defaults();
    let diagnostics = commas::check("[\"café, menu\", 3]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn respects_relaxed_spacing_config() {
    let cfg = Config::new_for_tests(-1, 0, -1);
    let diagnostics = commas::check("[1   ,2]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn handles_flow_mapping_entries() {
    let cfg = defaults();
    let diagnostics = commas::check("{foo: 1 ,bar: 2}\n", &cfg);
    assert_eq!(
        diagnostics,
        vec![
            Violation {
                line: 1,
                column: 8,
                message: "too many spaces before comma".to_string(),
            },
            Violation {
                line: 1,
                column: 10,
                message: "too few spaces after comma".to_string(),
            },
        ]
    );
}

#[test]
fn allows_comment_after_comma_on_same_line() {
    let cfg = defaults();
    let diagnostics = commas::check("[1, # comment\n  2]\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn allows_comment_after_comma_with_crlf() {
    let cfg = defaults();
    let diagnostics = commas::check("[1, # comment\r\n  2]\r\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn carriage_return_after_comma_is_ignored() {
    let cfg = defaults();
    let diagnostics = commas::check("[1,\r\n  2]\r\n", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn trailing_spaces_without_next_token_do_not_panic() {
    let cfg = defaults();
    let diagnostics = commas::check("[1,  ", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn bare_carriage_return_is_treated_as_line_break() {
    let cfg = defaults();
    let diagnostics = commas::check("[1, 2]\r", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn bare_carriage_return_before_comma_is_ignored() {
    let cfg = defaults();
    let diagnostics = commas::check("[1\r, 2]\r", &cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}
