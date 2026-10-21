//! `ryl format`'s re-indent: each line moves to where `indentation` expects it at the
//! target width, a block body moves as a whole to where `check-multi-line-strings` expects
//! it, continuations move with their owners, and a document that cannot be moved safely is
//! left byte-identical.

use std::ops::RangeInclusive;
use std::path::Path;
use std::time::Instant;

use ryl::config::YamlLintConfig;
use ryl::format::format_str;
use ryl::rules::indentation::{
    Cause, Config, IndentSequencesSetting, Refusal, SpacesSetting, check, fix, reindent,
};

fn target(width: usize) -> Config {
    Config::new(
        SpacesSetting::Fixed(width),
        IndentSequencesSetting::True,
        false,
    )
}

fn multi_line_target(width: usize) -> Config {
    Config::new(
        SpacesSetting::Fixed(width),
        IndentSequencesSetting::True,
        true,
    )
}

fn refusal(lines: RangeInclusive<usize>, cause: Cause) -> Vec<Refusal> {
    vec![Refusal { lines, cause }]
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
fn block_scalar_bodies_move_to_where_multi_line_checks_expect_them() {
    for (width, input, expected) in [
        (4, "block: |\n  text\n", "block: |\n    text\n"),
        (4, "- |\n  text\n", "- |\n      text\n"),
        (4, "? |\n  k\n: v\n", "? |\n      k\n: v\n"),
        (4, "a:\n  |\n  x\n", "a:\n    |\n        x\n"),
        (4, "a: |\n\n  x\n", "a: |\n\n    x\n"),
        (4, "a: |\n  x\n   \n  y\n", "a: |\n    x\n     \n    y\n"),
        (
            4,
            "a: |+\n  x\n\n   \nb: 1\n",
            "a: |+\n    x\n\n     \nb: 1\n",
        ),
        (2, "a: !!str |\n          deep\n", "a: !!str |\n  deep\n"),
        (4, "|\n2\n", "|\n    2\n"),
    ] {
        let out = reindent(input, &target(width));
        assert_eq!(
            (out.text.as_str(), out.refused),
            (expected, vec![]),
            "{input:?}"
        );
        assert_eq!(check(&out.text, &multi_line_target(width)), [], "{input:?}");
    }
}

#[test]
fn a_block_body_keeps_its_relative_indents_and_indicator() {
    assert_reindents(
        4,
        &[
            (
                "a: >\n  folded\n    more\n  back\n",
                "a: >\n    folded\n      more\n    back\n",
            ),
            ("a: |2\n    lead\n  x\n", "a: |2\n    lead\n  x\n"),
            (
                "k:\n  s: |-2\n     lead\n    x\n",
                "k:\n    s: |-2\n       lead\n      x\n",
            ),
        ],
    );
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
                "k:\n  s: |\n    text\n  # c\n  b: 1\n",
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
fn long_comment_runs_reindent_in_linear_time() {
    let measure = |count| {
        let input = format!(
            "first: value\n{}last: value\n",
            "# a plain comment\n".repeat(count)
        );
        let start = Instant::now();
        let output = reindent(&input, &target(2));
        assert_eq!(output.text, input);
        start.elapsed()
    };
    let small = (0..3).map(|_| measure(4_000)).min().unwrap();
    let large = (0..3).map(|_| measure(16_000)).min().unwrap();
    assert!(
        large <= small * 5 + std::time::Duration::from_millis(50),
        "comment-run scaling: 4,000 lines took {small:?}, 16,000 took {large:?}"
    );
}

#[test]
fn an_unsafe_document_is_refused_and_the_rest_still_move() {
    for (input, expected, refused) in [
        (
            "k:\n    - a\n    -\tb\n",
            "k:\n    - a\n    -\tb\n",
            refusal(1..=3, Cause::Tab),
        ),
        (
            "k:\n    a: 1\n \t\n    b: 2\n",
            "k:\n    a: 1\n \t\n    b: 2\n",
            refusal(1..=4, Cause::Tab),
        ),
        (
            ": value\n---\nk:\n    a: 1\n",
            ": value\n---\nk:\n  a: 1\n",
            refusal(1..=1, Cause::Unfollowable),
        ),
        (
            "k:\n    a: 1\n---\n: v\n---\nj:\n    b: 1\n",
            "k:\n  a: 1\n---\n: v\n---\nj:\n  b: 1\n",
            refusal(3..=4, Cause::Unfollowable),
        ),
        (
            "k:\n    a: 1  # yamllint disable-line rule:indentation\n---\nj:\n    b: 1\n",
            "k:\n    a: 1  # yamllint disable-line rule:indentation\n---\nj:\n  b: 1\n",
            refusal(1..=2, Cause::Disabled),
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

#[test]
fn changed_events_roll_back_only_the_unsafe_document() {
    let cfg = Config::new(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    for newline in ["\n", "\r\n", "\r"] {
        let input =
            "k:\n    a: 1\n---\na:\n  - a\nb: >\n   x\n  #\n".replace('\n', newline);
        let expected =
            "k:\n  a: 1\n---\na:\n  - a\nb: >\n   x\n  #\n".replace('\n', newline);
        let output = reindent(&input, &cfg);
        assert_eq!(output.text, expected);
        assert_eq!(output.refused, refusal(3..=8, Cause::Changed));
        assert_eq!(reindent(&output.text, &cfg), output);
    }
}

#[test]
fn changed_events_roll_back_a_single_document() {
    let input = "a:\n  - a\nb: >\n   x\n  #\n";
    let cfg = Config::new(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    let output = reindent(input, &cfg);
    assert_eq!(output.text, input);
    assert_eq!(output.refused, refusal(1..=5, Cause::Changed));
}

#[test]
fn indicator_gaps_close_and_what_hangs_on_them_follows() {
    assert_reindents(
        2,
        &[
            ("-   a: 1\n    b: 2\n", "- a: 1\n  b: 2\n"),
            ("- -   a: 1\n      b: 2\n", "- - a: 1\n    b: 2\n"),
            (
                "-   a: |2\n          lead\n        x\n",
                "- a: |2\n        lead\n      x\n",
            ),
            ("-   a: \"x\n      y\"\n", "- a: \"x\n    y\"\n"),
            ("?   k1: 1\n    k2: 2\n: v\n", "? k1: 1\n  k2: 2\n: v\n"),
            ("? a\n:   - x\n    - y\n", "? a\n: - x\n  - y\n"),
        ],
    );
}

#[test]
fn a_directive_on_the_rule_owning_a_gap_refuses_its_document() {
    for (input, lines) in [
        (
            "-   a: 1  # yamllint disable-line rule:hyphens\n    b: 2\n",
            1..=2,
        ),
        (
            "# yamllint disable rule:colons\n? a\n:   - x\n    - y\n",
            1..=4,
        ),
    ] {
        let out = reindent(input, &target(2));
        assert_eq!(
            (out.text.as_str(), out.refused),
            (input, refusal(lines, Cause::Disabled))
        );
    }
}

#[test]
fn sequences_can_sit_flush_with_their_key() {
    let flush = Config::new(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    let out = reindent("k:\n  - a\n  - b:\n      - c\n", &flush);
    assert_eq!(out.text, "k:\n- a\n- b:\n  - c\n");
    assert_eq!(check(&out.text, &flush), []);
}

#[test]
fn a_block_mapping_joins_its_dash_or_breaks_from_it() {
    let joined = target(2).with_dash_on_own_line(false);
    let broken = target(2).with_dash_on_own_line(true);
    for (cfg, input, expected) in [
        (
            joined,
            "items:\n-\n    name: web\n    port: 80\n",
            "items:\n  - name: web\n    port: 80\n",
        ),
        (joined, "- &x\n  a: 1\n", "- &x\n  a: 1\n"),
        (joined, "- # c\n  a: 1\n", "- # c\n  a: 1\n"),
        (joined, "-\n a: 1\n b: 2\n", "-\n  a: 1\n  b: 2\n"),
        (broken, "- a: 1\n  b: 2\n", "-\n  a: 1\n  b: 2\n"),
        (broken, "- a: \"b\"  # c\n", "- a: \"b\"  # c\n"),
        (broken, "- a: z'  # c\n", "- a: z'  # c\n"),
        (broken, "- a: {b: c'}\t#d\n", "- a: {b: c'}\t#d\n"),
        (broken, "k:\r- a: 1", "k:\r  -\r    a: 1"),
        (
            broken,
            "- a: 1  # yamllint disable-line rule:hyphens\n",
            "- a: 1  # yamllint disable-line rule:hyphens\n",
        ),
    ] {
        let out = reindent(input, &cfg);
        assert_eq!(
            (out.text.as_str(), out.refused),
            (expected, vec![]),
            "{input:?}"
        );
    }
}

#[test]
fn large_reindent_keeps_the_strictly_valid_result() {
    let mut input = String::from("root:\n");
    let mut expected = input.clone();
    for index in 0..55_000 {
        let entry = format!("key_{index:05}_long: value\n");
        input.push_str("    ");
        input.push_str(&entry);
        expected.push_str("  ");
        expected.push_str(&entry);
    }
    assert!(expected.len() >= 1_000_000);
    let result = reindent(&input, &target(2));
    assert_eq!(result.text, expected);
    assert!(result.refused.is_empty());
    assert_eq!(reindent(&result.text, &target(2)).text, expected);
    let cfg = YamlLintConfig::from_toml_str("indent-width = 2\n[format]\n").unwrap();
    assert_eq!(
        format_str(&input, &cfg, Path::new("large.yaml"), &[]),
        expected
    );
}

#[test]
fn large_reindent_retains_single_document_rollback() {
    let input = format!("# {}\na:\n  - a\nb: >\n   x\n  #\n", "x".repeat(1_000_000));
    let cfg = Config::new(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    let output = reindent(&input, &cfg);
    assert_eq!(output.text, input);
    assert_eq!(output.refused, refusal(1..=6, Cause::Changed));
}

#[test]
fn large_multi_document_reindent_keeps_strict_verification_and_rollback() {
    let prefix = format!("# {}\n", "x".repeat(1_000_000));
    for (suffix, expected, refused) in [
        ("z:\n    b: 2\n", "z:\n  b: 2\n", vec![]),
        (
            "a:\n  - a\nb: >\n   x\n  #\n",
            "a:\n  - a\nb: >\n   x\n  #\n",
            refusal(4..=9, Cause::Changed),
        ),
    ] {
        let input = format!("{prefix}k:\n    a: 1\n---\n{suffix}");
        let expected = format!("{prefix}k:\n  a: 1\n---\n{expected}");
        let cfg = Config::new(
            SpacesSetting::Fixed(2),
            IndentSequencesSetting::False,
            false,
        );
        let output = reindent(&input, &cfg);
        assert_eq!(output.text, expected);
        assert_eq!(output.refused, refused);
        assert_eq!(reindent(&output.text, &cfg).text, expected);
    }
}

#[test]
fn large_relaxed_only_reindent_keeps_the_final_strict_guard_required() {
    let prefix = format!("# {}\n", "x".repeat(1_000_000));
    let input = format!("{prefix}key: [\n  value\n  ]\n");
    let output = reindent(&input, &target(0));
    assert_eq!(
        output.text.strip_prefix(&prefix),
        Some("key: [\nvalue\n ]\n")
    );
    assert!(output.refused.is_empty());
    assert!(
        ryl_yaml_parser::Parser::new_from_str_with_options(
            &output.text,
            ryl_yaml_parser::options! { strict_indentation: true },
        )
        .collect::<Result<Vec<_>, _>>()
        .is_err()
    );
}
