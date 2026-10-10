//! Without `indent-width`, `ryl format` keeps the indent width that moves the fewest lines
//! led by a token, and `line-ending = "auto"` keeps the ending most lines use; a tie or no
//! evidence falls back to 2 and `lf`.

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
fn block_scalar_bodies_never_sway_the_width() {
    let body = "        text\n".repeat(20);
    let mostly_two =
        format!("a:\n  b: 1\n  c: 2\n  d: 3\nx:\n    y: |\n{body}    z: 1\n");
    let mostly_four = format!(
        "a:\n    b: 1\n    c: 2\n    d: 3\nx:\n  y: |\n{}  z: 1\n",
        "    text\n".repeat(20)
    );
    let unset = config(UNSET);
    assert_eq!(file_indent_width(&unset, &mostly_two), 2);
    assert_eq!(file_indent_width(&unset, &mostly_four), 4);
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
