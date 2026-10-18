use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::format_str;

fn config(toml: &str) -> YamlLintConfig {
    YamlLintConfig::from_toml_str(toml).expect("detection config parses")
}

fn format(input: &str, toml: &str) -> String {
    format_str(input, &config(toml), Path::new("a.yaml"), &[])
}

const UNSET: &str = "[format]\n";

const MOSTLY_FOUR: &str =
    "a:\n    b:\n        c: 1\n    d: 2\nx:\n  y: 1\ne:\n    f: 1\n";
const MOSTLY_TWO: &str = "a:\n  b:\n    c: 1\n  d: 2\nx:\n    y: 1\ne:\n  f: 1\n";

#[test]
fn unset_width_reindents_to_two_and_configured_width_overrides() {
    let two = "a:\n  b:\n    c: 1\n  d: 2\nx:\n  y: 1\ne:\n  f: 1\n";
    for input in [MOSTLY_FOUR, MOSTLY_TWO] {
        assert_eq!(format(input, UNSET), two);
        assert_eq!(format(input, UNSET), format(input, "indent-width = 2\n"));
    }
    assert_eq!(
        format(MOSTLY_FOUR, "indent-width = 3\n"),
        "a:\n   b:\n      c: 1\n   d: 2\nx:\n   y: 1\ne:\n   f: 1\n"
    );
}

#[test]
fn default_width_moves_explicit_scalar_content_and_comments_safely() {
    let input = "root:\n    # scalar\n    scalar: |4\n        value\n";
    let expected = "root:\n  # scalar\n  scalar: |4\n      value\n";
    assert_eq!(format(input, UNSET), expected);
    assert_eq!(format(expected, UNSET), expected);
    assert_eq!(format(input, "indent-width = 4\n"), input);
}

#[test]
fn default_width_rolls_back_a_new_consistency_refusal() {
    let input =
        "a:\n    b: 1  # ryl disable-line rule:indentation\n---\nc:\n    d: 2\n";
    assert_eq!(format(input, UNSET), input);
    assert_eq!(
        format(input, "indent-width = 2\n"),
        "a:\n    b: 1  # ryl disable-line rule:indentation\n---\nc:\n  d: 2\n"
    );
}

#[test]
fn auto_keeps_the_line_ending_most_lines_use() {
    let auto = "[format]\nline-ending = 'auto'\n";
    for (input, expected) in [
        ("a: 1\r\nb: 2\r\nc: 3\n", "a: 1\r\nb: 2\r\nc: 3\r\n"),
        ("a: 1\nb: 2\nc: 3\r\n", "a: 1\nb: 2\nc: 3\n"),
        ("a: 1\r\nb: 2\n", "a: 1\nb: 2\n"),
        ("a: 1", "a: 1\n"),
    ] {
        assert_eq!(format(input, auto), expected, "{input:?}");
    }
    assert_eq!(format("a: 1\r\nb: 2\r\n", UNSET), "a: 1\nb: 2\n");
}
