use regex::Regex;
use ryl::config::YamlLintConfig;
use ryl::directives::PerLineRuleApply;
use ryl::rules::key_ordering::{self, Config};

fn config(options: &str) -> Config {
    let yaml = format!("rules:\n  key-ordering:{options}\n");
    Config::resolve(&YamlLintConfig::from_yaml_str(&yaml).expect("config parses"))
}

fn fix(input: &str) -> Option<String> {
    key_ordering::fix(input, &config(" enable"), &[])
}

fn reasons(input: &str) -> Vec<(usize, usize, String)> {
    key_ordering::unfixed(input, &config(" enable"), &[])
        .into_iter()
        .map(|violation| (violation.line, violation.column, violation.message))
        .collect()
}

#[test]
fn sorts_entries_with_their_comments() {
    let cases = [
        (
            "---\n# Gamma setting.\ngamma: 3  # Gamma-specific note.\n# Alpha setting.\nalpha: 1  # Alpha-specific note.\nbeta: 2\n",
            "---\n# Alpha setting.\nalpha: 1  # Alpha-specific note.\nbeta: 2\n# Gamma setting.\ngamma: 3  # Gamma-specific note.\n",
        ),
        ("b: 1\na: 2\n  # about a\n", "a: 2\n  # about a\nb: 1\n"),
        (
            "b:\n  - x\n  # about b\na: 1\n",
            "a: 1\nb:\n  - x\n  # about b\n",
        ),
        ("b: 1\na: 2", "a: 2\nb: 1"),
        ("b: 1\r\na: 2\n", "a: 2\r\nb: 1\n"),
        ("b: 1\ra: 2\r", "a: 2\rb: 1\r"),
        ("\u{feff}b: 1\na: 2\n", "\u{feff}a: 2\nb: 1\n"),
        ("- b: 1\n  a: 2\n", "- a: 2\n  b: 1\n"),
        (
            "k:\n- b: [1,\n    2]\n  a: !t x\n",
            "k:\n- a: !t x\n  b: [1,\n    2]\n",
        ),
        ("b: >\n  folded\n\n\na: 1\n", "a: 1\n\n\nb: >\n  folded\n"),
        ("c: 1\nb: 2\nb: 3\na: 4\n", "a: 4\nb: 2\nb: 3\nc: 1\n"),
        ("x: 0\n<<: {y: 1}\n", "<<: {y: 1}\nx: 0\n"),
        ("d:\n  - ? e\n# lead\na: 1\n", "# lead\na: 1\nd:\n  - ? e\n"),
        ("b: [&p 1, *p]\na: 2\n", "a: 2\nb: [&p 1, *p]\n"),
        ("x: &a 1\ny: *a\nb: 0\n", "b: 0\nx: &a 1\ny: *a\n"),
        (
            "b: 1  # ryl disable-line rule:truthy\n# ryl disable-line rule:line-length\na: 2\n",
            "# ryl disable-line rule:line-length\na: 2\nb: 1  # ryl disable-line rule:truthy\n",
        ),
        (
            "z:\n  b: 1\n  a: 2\n\n# about y\n\ny: 0\n",
            "z:\n  a: 2\n  b: 1\n\n# about y\n\ny: 0\n",
        ),
        (
            "b:\n  d: 1\n  c: 2\na: |\n  text\n",
            "a: |\n  text\nb:\n  c: 2\n  d: 1\n",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(fix(input).as_deref(), Some(expected), "input: {input:?}");
        assert_eq!(fix(expected), None, "not idempotent: {expected:?}");
    }
}

#[test]
fn ignored_keys_hold_their_slots_and_locale_ranks_the_rest() {
    let ignored = config("\n    ignored-keys: [\"^x-\"]");
    assert_eq!(
        key_ordering::fix("c: 1\nx-a: 0\nb: 2\na: 3\n", &ignored, &[]).as_deref(),
        Some("a: 3\nx-a: 0\nb: 2\nc: 1\n")
    );
    let cfg = YamlLintConfig::from_yaml_str(
        "locale: en_US.UTF-8\nrules:\n  key-ordering: enable\n",
    )
    .expect("config parses");
    assert_eq!(
        key_ordering::fix("B: 1\na: 2\n", &Config::resolve(&cfg), &[]).as_deref(),
        Some("a: 2\nB: 1\n")
    );
}

#[test]
fn leaves_unsafe_mappings_unsorted_with_a_reason() {
    let cases = [
        ("{b: 1, a: 2}\n", 1, 8, "flow mappings are not sorted"),
        (
            "!t\nb: 1\na: 2\n",
            3,
            1,
            "the mapping is tagged or part of a key",
        ),
        (
            "? b: 1\n  a: 2\n: v\n",
            2,
            3,
            "the mapping is tagged or part of a key",
        ),
        (
            "&x b: 1\na: 2\n",
            2,
            1,
            "a key is complex, explicit, or has an anchor or tag",
        ),
        (
            "? b\n: 1\na: 2\n",
            3,
            1,
            "a key is complex, explicit, or has an anchor or tag",
        ),
        (
            "b: 1\n# loose\n\na: 2\n",
            4,
            1,
            "a comment between entries is attached to neither",
        ),
        (
            "b: 1\n  # t\n\n  # loose\na: 2\n",
            5,
            1,
            "a comment between entries is attached to neither",
        ),
        (
            "b: |+\n  t\n\na: 1\n",
            4,
            1,
            "an entry ends in a keep-chomping block scalar",
        ),
        (
            "b: 1\n# ryl disable-line rule:truthy # ryl disable rule:commas\na: 2\n",
            3,
            1,
            "a suppression directive applies inside it",
        ),
        (
            "b: 1  # ryl disable-line rule:key-ordering\na: 2\n",
            2,
            1,
            "a suppression directive applies inside it",
        ),
        (
            "b: 1\n  # ryl disable-line rule:truthy\na: 2\n",
            3,
            1,
            "a suppression directive applies inside it",
        ),
        (
            "k:\n# ryl disable-line rule:truthy\n  b: 1\n  a: 2\n",
            4,
            3,
            "a suppression directive applies inside it",
        ),
        (
            "- d: 1\n  # about c\n  c: 2\n",
            3,
            3,
            "an entry's leading comment would land on a `- ` line",
        ),
        (
            "x: &a 1\nb: *a\na: 2\n",
            2,
            1,
            "an alias would move before its anchor or a redefinition",
        ),
        (
            "true: first\nTrue: last\n",
            2,
            1,
            "two differently spelled keys load as the same key",
        ),
        (
            "b: |\n  t\n    \na: 1\n",
            4,
            1,
            "the sorted output failed verification",
        ),
    ];
    for (input, line, column, reason) in cases {
        assert_eq!(fix(input), None, "input: {input:?}");
        assert_eq!(
            reasons(input),
            vec![(line, column, reason.to_owned())],
            "input: {input:?}"
        );
    }
}

#[test]
fn reports_nothing_for_ordered_suppressed_or_unparsable_input() {
    for input in [
        "a: 1\nb: 2\n",
        "b: 1\na: 2  # ryl disable-line rule:key-ordering\n",
        "b: *missing\n: [\n",
    ] {
        assert_eq!(fix(input), None, "input: {input:?}");
        assert!(reasons(input).is_empty(), "input: {input:?}");
    }
}

#[test]
fn declines_a_sort_that_moves_a_line_into_a_per_line_ignore() {
    let regex = Regex::new("^  b").expect("regex compiles");
    let rules = [key_ordering::ID];
    let per_line = [PerLineRuleApply {
        regex: Some(&regex),
        rules: Some(&rules),
    }];
    let input = "- b: 1\n  a: 2\n";
    assert_eq!(
        key_ordering::fix(input, &config(" enable"), &per_line),
        None
    );
    assert_eq!(
        key_ordering::unfixed(input, &config(" enable"), &per_line)[0].message,
        "the sorted output failed verification"
    );
}
