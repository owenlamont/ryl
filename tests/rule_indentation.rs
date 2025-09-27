use ryl::rules::indentation::{self, Config, IndentSequencesSetting, SpacesSetting, Violation};

fn config(spaces: SpacesSetting, indent_sequences: IndentSequencesSetting, multi: bool) -> Config {
    Config::new_for_tests(spaces, indent_sequences, multi)
}

#[test]
fn detects_unindented_sequence_in_mapping() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n- item\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 1,
            message: "wrong indentation: expected 2 but found 0".to_string(),
        }]
    );
}

#[test]
fn allows_unindented_sequence_when_disabled() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    let yaml = "root:\n- item\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn detects_indented_sequence_when_disabled() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    let yaml = "root:\n  - item\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 3,
            message: "wrong indentation: expected 0 but found 2".to_string(),
        }]
    );
}

#[test]
fn enforces_consistent_spacing() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n   child: value\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 4,
            message: "wrong indentation: expected 2 but found 3".to_string(),
        }]
    );
}

#[test]
fn checks_multiline_strings_when_enabled() {
    let cfg = config(SpacesSetting::Fixed(4), IndentSequencesSetting::True, true);
    let yaml = "quote: |\n    good\n     bad\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 3,
            column: 6,
            message: "wrong indentation: expected 4but found 5".to_string(),
        }]
    );
}

#[test]
fn multiline_strings_ignored_when_disabled() {
    let cfg = config(SpacesSetting::Fixed(4), IndentSequencesSetting::True, false);
    let yaml = "quote: |\n    good\n     bad\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn folded_multiline_reports_violation() {
    let cfg = config(SpacesSetting::Fixed(4), IndentSequencesSetting::True, true);
    let yaml = "quote: >\n    good\n     bad\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 3,
            column: 6,
            message: "wrong indentation: expected 4but found 5".to_string(),
        }]
    );
}

#[test]
fn consistent_spaces_detects_violation() {
    let cfg = config(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let yaml = "root:\n  child:\n    grand: 1\n   bad: 2\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 4,
            column: 4,
            message: "wrong indentation: expected 4 but found 3".to_string(),
        }]
    );
}

#[test]
fn multiline_resets_context_after_block() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, true);
    let yaml = "quote: |\n  text\nnext: value\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn indent_sequences_consistent_detects_mixed_styles() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::Consistent,
        false,
    );
    let yaml = "root:\n- top\nanother:\n  - inner\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 4,
            column: 3,
            message: "wrong indentation: expected 0 but found 2".to_string(),
        }]
    );
}

#[test]
fn indent_sequences_whatever_allows_both_styles() {
    let cfg = config(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::Whatever,
        false,
    );
    let yaml = "root:\n- top\nanother:\n  - inner\n";
    let hits = indentation::check(yaml, &cfg);
    assert!(hits.is_empty());
}

#[test]
fn tab_indentation_is_counted() {
    let cfg = config(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    let yaml = "root:\n\tchild: value\n";
    let hits = indentation::check(yaml, &cfg);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 2,
            message: "wrong indentation: expected 2 but found 1".to_string(),
        }]
    );
}
