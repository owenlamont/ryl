use ryl::rules::trailing_spaces::{self, Violation};

#[test]
fn reports_trailing_space() {
    let input = "---\nsome: text \n";
    let hits = trailing_spaces::check(input);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 11,
        }]
    );
}

#[test]
fn reports_trailing_tab() {
    let input = "key:\t\n";
    let hits = trailing_spaces::check(input);
    assert_eq!(hits, vec![Violation { line: 1, column: 5 }]);
}

#[test]
fn ignores_clean_lines() {
    let input = "foo: bar\n";
    let hits = trailing_spaces::check(input);
    assert!(hits.is_empty());
}

#[test]
fn handles_crlf_lines() {
    let input = "---\r\nsome: text \r\n";
    let hits = trailing_spaces::check(input);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 11,
        }]
    );
}
