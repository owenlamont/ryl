//! `ryl format`'s re-indent: each line moves to where `indentation` expects it at the
//! target width, block bodies and continuations move with their owners, and a document
//! that cannot be moved safely is left byte-identical.

use std::ops::RangeInclusive;

use ryl::rules::indentation::{
    Config, IndentSequencesSetting, Refusal, SpacesSetting, check, fix, reindent,
};

fn target(width: usize) -> Config {
    Config::new(
        SpacesSetting::Fixed(width),
        IndentSequencesSetting::True,
        false,
    )
}

fn refusal(lines: RangeInclusive<usize>, disabled: bool) -> Vec<Refusal> {
    vec![Refusal { lines, disabled }]
}

fn assert_reindents(width: usize, cases: &[(&str, &str)]) {
    let cfg = target(width);
    for &(input, expected) in cases {
        let out = reindent(input, &cfg);
        assert_eq!(out.text, expected, "{input:?}");
        assert_eq!(out.refused, [], "{input:?}");
        assert_eq!(check(&out.text, &cfg), [], "{input:?}");
    }
}

#[test]
fn collections_move_to_the_target_width() {
    assert_reindents(
        2,
        &[
            (
                "---\nmap:\n    nested:\n          deep: x\n    other: 1\nseq:\n- a\n- b\n",
                "---\nmap:\n  nested:\n    deep: x\n  other: 1\nseq:\n  - a\n  - b\n",
            ),
            (
                "%YAML 1.2\n---\nk:\n    a: 1\n...\n---\nj:\n      b: 2\n",
                "%YAML 1.2\n---\nk:\n  a: 1\n...\n---\nj:\n  b: 2\n",
            ),
            (
                "k:\n    ? complex key\n    : value\n    ? [a, b]\n    : v2\n",
                "k:\n  ? complex key\n  : value\n  ? [a, b]\n  : v2\n",
            ),
            (
                "k:\n    a: &x\n        x: 1\n    c: !!map\n        y: 2\n",
                "k:\n  a: &x\n    x: 1\n  c: !!map\n    y: 2\n",
            ),
        ],
    );
    assert_reindents(4, &[("k:\n  a:\n  - b\n", "k:\n    a:\n        - b\n")]);
}

#[test]
fn block_scalar_bodies_move_with_their_owner() {
    assert_reindents(
        2,
        &[
            (
                "k:\n    s: |2\n          lead\n        x\n",
                "k:\n  s: |2\n        lead\n      x\n",
            ),
            (
                "k:\n    s: |\n      text\n\n        \n      more\n",
                "k:\n  s: |\n    text\n\n      \n    more\n",
            ),
            (
                "k:\n    s: |\n        text\n      # c\n    b: 1\n",
                "k:\n  s: |\n      text\n  # c\n  b: 1\n",
            ),
        ],
    );
}

#[test]
fn continuations_shift_and_flow_lines_align() {
    assert_reindents(
        2,
        &[
            (
                "k:\n    p: plain first\n      continued here\n    q: \"dq first\n       continued\"\n",
                "k:\n  p: plain first\n    continued here\n  q: \"dq first\n     continued\"\n",
            ),
            (
                "k:\n    f: [a,\n        b,\n      c]\n    m: {x: 1,\n          y: 2}\n",
                "k:\n  f: [a,\n      b,\n      c]\n  m: {x: 1,\n      y: 2}\n",
            ),
            ("k: [a,\nb]\n", "k: [a,\n    b]\n"),
        ],
    );
}

#[test]
fn whole_line_comments_follow_the_line_they_sit_against() {
    assert_reindents(
        2,
        &[(
            "k:\n    # leading\n    a: 1  # trailing\n        # odd comment\n    b: 2\n# top\n",
            "k:\n  # leading\n  a: 1  # trailing\n  # odd comment\n  b: 2\n# top\n",
        )],
    );
}

#[test]
fn an_unsafe_document_is_refused_and_the_rest_still_move() {
    for (input, expected, refused) in [
        (
            "k:\n    - a\n    -\tb\n",
            "k:\n    - a\n    -\tb\n",
            refusal(1..=3, false),
        ),
        (
            "k:\n    a: 1\n \t\n    b: 2\n",
            "k:\n    a: 1\n \t\n    b: 2\n",
            refusal(1..=4, false),
        ),
        (
            ": value\n---\nk:\n    a: 1\n",
            ": value\n---\nk:\n  a: 1\n",
            refusal(1..=1, false),
        ),
        (
            "k:\n    a: 1\n---\n: v\n---\nj:\n    b: 1\n",
            "k:\n  a: 1\n---\n: v\n---\nj:\n  b: 1\n",
            refusal(3..=4, false),
        ),
        (
            "k:\n    a: 1  # yamllint disable-line rule:indentation\n---\nj:\n    b: 1\n",
            "k:\n    a: 1  # yamllint disable-line rule:indentation\n---\nj:\n  b: 1\n",
            refusal(1..=2, true),
        ),
    ] {
        let out = reindent(input, &target(2));
        assert_eq!(
            (out.text.as_str(), out.refused),
            (expected, refused),
            "{input:?}"
        );
    }
}

#[test]
fn nothing_to_move_is_no_fix() {
    for input in ["k:\n  a: 1\n", "k: [\n", ""] {
        assert_eq!(fix(input, &target(2)), None, "{input:?}");
    }
    assert_eq!(
        fix("k:\n    a: 1\n", &target(2)).as_deref(),
        Some("k:\n  a: 1\n")
    );
}
