//! Automatic layout detection prefers consistent indentation, then fewer changed lines;
//! line endings follow the majority, with ties falling back to 2 and `lf`.

use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::{file_indent_width, format_str};

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
fn the_width_most_lines_already_use_wins() {
    assert_eq!(
        format(MOSTLY_FOUR, UNSET),
        "a:\n    b:\n        c: 1\n    d: 2\nx:\n    y: 1\ne:\n    f: 1\n"
    );
    assert_eq!(
        format(MOSTLY_TWO, UNSET),
        "a:\n  b:\n    c: 1\n  d: 2\nx:\n  y: 1\ne:\n  f: 1\n"
    );
    assert_eq!(
        format(MOSTLY_FOUR, "indent-width = 3\n"),
        "a:\n   b:\n      c: 1\n   d: 2\nx:\n   y: 1\ne:\n   f: 1\n"
    );
}

#[test]
fn a_tie_or_a_flat_file_falls_back_to_two() {
    let unset = config(UNSET);
    for input in [
        "a:\n  b: 1\nx:\n    y: 1\n",
        "a: 1\nb: [1, 2]\n",
        "- a\n- b\n",
        "",
    ] {
        assert_eq!(file_indent_width(&unset, input), 2, "{input:?}");
    }
    assert_eq!(
        format("a:\n  b: 1\nx:\n    y: 1\n", UNSET),
        "a:\n  b: 1\nx:\n  y: 1\n"
    );
}

#[test]
fn explicit_key_content_constrains_detected_width() {
    for input in ["? a\n: a\nb:\n     - a\n", "? a\n: -  a: a\n     b: b\n"] {
        assert_eq!(file_indent_width(&config(UNSET), input), 2, "{input:?}");
    }
    for input in [
        "a:\n    b: 1\n? a\n: b\nc:\n    d: 1\n",
        "? a\n:\n     b\nc:\n    d: 1\ne:\n    f: 1\n",
    ] {
        assert_eq!(file_indent_width(&config(UNSET), input), 4, "{input:?}");
    }
    let flow = "x: {? a: b}\ny:\n     z: value\n";
    assert_eq!(file_indent_width(&config(UNSET), flow), 5);
}

#[test]
fn block_scalar_bodies_and_comments_count_as_changed_lines() {
    let unset = config(UNSET);
    let scalar = format!("    y: |\n{}    z: 1\n", "        text\n".repeat(20));
    let comments = format!("{}    y: 1\n", "    # text\n".repeat(20));
    for nested in [scalar, comments] {
        let input = format!("a:\n  b: 1\n  c: 2\n  d: 3\nx:\n{nested}");
        assert_eq!(file_indent_width(&unset, &input), 4, "{input}");
        assert_eq!(
            format(&input, UNSET),
            format(&input, "indent-width = 4\n"),
            "{input}"
        );
    }
}

#[test]
fn a_dash_join_that_enables_another_is_scored_once_settled() {
    let input = "a:\n  -\n   b:\n   -\n     c: 1\nx:\n    y: 1\n";
    assert_eq!(file_indent_width(&config(UNSET), input), 4);
    assert_eq!(
        format(input, UNSET),
        "a:\n    - b:\n          - c: 1\nx:\n    y: 1\n"
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
