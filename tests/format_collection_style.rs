//! `[format] sequence-style` / `mapping-style` from the collection-style probe corpus.
//! PyYAML, ruamel, the `yaml` npm package and granit load each row's `expected` to its
//! input's events, collection style aside; every input left unchanged is one where one
//! of those parsers reads the other style differently, or the rewrite would lose a
//! comment or a line break or, under `flow`, outgrow `line-length`.

use std::fs;
use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::{FORMAT_RULE_IDS, format_str, refusals};
use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

const BOTH: &str = "[format]\nsequence-style = 'block'\nmapping-style = 'block'\n";
const SEQUENCES: &str = "[format]\nsequence-style = 'block'\n";
const MAPPINGS: &str = "[format]\nmapping-style = 'block'\n";

fn config(toml: &str) -> YamlLintConfig {
    YamlLintConfig::from_toml_str(toml).expect("collection-style config parses")
}

/// `input` after `ryl format` with every pass but `braces` and `brackets` skipped.
fn restyle(input: &str, toml: &str) -> String {
    let others: Vec<&str> = FORMAT_RULE_IDS
        .into_iter()
        .filter(|rule| !["braces", "brackets"].contains(rule))
        .collect();
    format_str(input, &config(toml), Path::new("golden.yaml"), &others)
}

const CASES: [(&str, &str, &str, &str); 36] = [
    (
        "issue",
        BOTH,
        "items: [one, two, three]\nmetadata: {name: example, enabled: true}\n",
        "items:\n  - one\n  - two\n  - three\nmetadata:\n  name: example\n  enabled: true\n",
    ),
    (
        "properties",
        BOTH,
        "k: &x !!seq [a, b]\nm: &y {a: 1}\nr: *x\ns: [*y, &z c, !!str 1]\n",
        "k: &x !!seq\n  - a\n  - b\nm: &y\n  a: 1\nr: *x\ns:\n  - *y\n  - &z c\n  - !!str 1\n",
    ),
    (
        "anchored-entry",
        BOTH,
        "k: [&x {a: 1}]\n",
        "k:\n  - &x\n    a: 1\n",
    ),
    (
        "tagged-entry",
        BOTH,
        "k: [!!map {a: 1}]\n",
        "k:\n  - !!map\n    a: 1\n",
    ),
    (
        "nested",
        BOTH,
        "x: [{a: 1}, {b: 2}]\n",
        "x:\n  - a: 1\n  - b: 2\n",
    ),
    (
        "sequences-only",
        SEQUENCES,
        "x: [{a: 1}, {b: 2}]\n",
        "x:\n  - {a: 1}\n  - {b: 2}\n",
    ),
    (
        "mappings-only",
        MAPPINGS,
        "x: [{a: 1}, {b: 2}]\n",
        "x: [{a: 1}, {b: 2}]\n",
    ),
    (
        "partial",
        MAPPINGS,
        "y: {p: [1, 2], q: {r: s}}\n",
        "y:\n  p: [1, 2]\n  q:\n    r: s\n",
    ),
    ("null-values", BOTH, "k: {a, b: }\n", "k:\n  a:\n  b:\n"),
    (
        "explicit-key",
        BOTH,
        "k: {? c, a: b, ? d : e}\n",
        "k:\n  c:\n  a: b\n  d: e\n",
    ),
    ("explicit-pair", BOTH, "k: [? a]\n", "k:\n  - a:\n"),
    (
        "json-key",
        BOTH,
        "m: {\"c\":3}\nn: [\"a\":1]\n",
        "m:\n  \"c\": 3\nn:\n  - \"a\": 1\n",
    ),
    (
        "alias-key",
        BOTH,
        "x: &a k\ny: {*a : 1}\n",
        "x: &a k\ny:\n  *a : 1\n",
    ),
    ("root", BOTH, "[a, b]\n", "- a\n- b\n"),
    ("root-mapping", BOTH, "{a: 1}\n", "a: 1\n"),
    (
        "root-after-marker",
        BOTH,
        "--- [a, b]\n--- !!map {c: 1}\n",
        "---\n- a\n- b\n--- !!map\nc: 1\n",
    ),
    (
        "trailing-comment",
        BOTH,
        "k: [a, b]  # c\n",
        "k:  # c\n  - a\n  - b\n",
    ),
    (
        "anchor-and-comment",
        BOTH,
        "k: &x [a]  # c\n",
        "k: &x  # c\n  - a\n",
    ),
    (
        "compact",
        BOTH,
        "- [a, b]\n- {c: 1, d: 2}\n",
        "- - a\n  - b\n- c: 1\n  d: 2\n",
    ),
    ("own-line", BOTH, "k:\n  [a, b]\n", "k:\n  - a\n  - b\n"),
    ("own-line-anchor", BOTH, "k: &x\n  [a]\n", "k: &x\n  - a\n"),
    ("dash-anchor", BOTH, "- &x [a]\n", "- &x\n  - a\n"),
    (
        "dash-gap",
        BOTH,
        "-  [a, b]\n-   {c: 1}\n",
        "- - a\n  - b\n- c: 1\n",
    ),
    (
        "pairs",
        BOTH,
        "k: [a: 1, b: 2]\n",
        "k:\n  - a: 1\n  - b: 2\n",
    ),
    (
        "in-sequence-mapping",
        BOTH,
        "- k: [a, b]\n  j: {x: 1}\n",
        "- k:\n    - a\n    - b\n  j:\n    x: 1\n",
    ),
    (
        "unindented-parent",
        BOTH,
        "k:\n- [a, b]\n",
        "k:\n- - a\n  - b\n",
    ),
    ("tab-separator", BOTH, "k: [a,\tb]\n", "k:\n  - a\n  - b\n"),
    (
        "duplicate-keys",
        BOTH,
        "k: {a: 1, a: 2}\n",
        "k:\n  a: 1\n  a: 2\n",
    ),
    (
        "plain-indicators",
        BOTH,
        "k: [a:b, -a, :b, a#b]\n",
        "k:\n  - a:b\n  - -a\n  - :b\n  - a#b\n",
    ),
    (
        "yaml-1.1",
        BOTH,
        "%YAML 1.1\n---\nk: [yes, 0o7, 010]\n",
        "%YAML 1.1\n---\nk:\n  - yes\n  - 0o7\n  - 010\n",
    ),
    (
        "empty-entries",
        BOTH,
        "k: [{}, [], {a: []}]\n",
        "k:\n  - {}\n  - []\n  - a: []\n",
    ),
    (
        "trailing-comma",
        BOTH,
        "k: [ a , b, ]\n",
        "k:\n  - a\n  - b\n",
    ),
    (
        "entries-on-lines",
        BOTH,
        "k: [\n  a,\n  b\n]\n",
        "k:\n  - a\n  - b\n",
    ),
    ("set", BOTH, "k: !!set {a, b}\n", "k: !!set\n  a:\n  b:\n"),
    (
        "crlf",
        BOTH,
        "k: [a]\r\nj: {c: d}\r\n",
        "k:\r\n  - a\r\nj:\r\n  c: d\r\n",
    ),
    (
        "deep",
        BOTH,
        "a: {b: {c: [d]}}\n",
        "a:\n  b:\n    c:\n      - d\n",
    ),
];

#[test]
fn golden_rewrites() {
    for (name, toml, input, expected) in CASES {
        assert_eq!(restyle(input, toml), expected, "{name}");
        assert_eq!(restyle(expected, toml), expected, "{name} is idempotent");
    }
}

#[test]
fn indent_width_sets_the_block_indent() {
    let toml = format!("indent-width = 4\n{BOTH}");
    assert_eq!(restyle("k: {a: [b]}\n", &toml), "k:\n    a:\n        - b\n");
}

#[test]
fn unsafe_collections_stay_flow_with_the_reason() {
    for (input, reason) in [
        ("k: [a\n  b, c]\n", "an entry spans lines"),
        ("k: [\"a\n  b\", c]\n", "an entry spans lines"),
        ("k: [a,  # c\n  b]\n", "it holds a comment"),
        ("k: [a, {b: 1,  # c\n  d: 2}]\n", "it holds a comment"),
        ("k: {? [a]: b}\n", "a key is a collection"),
        ("k: {: v}\n", "a key is empty"),
        ("[a]: b\n", "it is a mapping key"),
        ("k: {a:, b: 2}\n", "a plain key's `:` has no space after it"),
        ("[?x, -y]\n", "an entry starts with `?`"),
        ("k: {?x: 1}\n", "an entry starts with `?`"),
        ("k: [&x ?y]\n", "an entry starts with `?`"),
        ("-  a: [x]\n", "an indicator before it has extra spaces"),
        (
            "- [a]  # c\n",
            "a trailing comment has no key line to move to",
        ),
        (
            "[a]  # c\n",
            "a trailing comment has no key line to move to",
        ),
        (
            "? k\n: [a]  # c\n",
            "a trailing comment has no key line to move to",
        ),
        (
            "k: [a,\n  b]  # c\n",
            "a trailing comment has no key line to move to",
        ),
        (
            "k:\n  -  a: [x]\n",
            "an indicator before it has extra spaces",
        ),
        ("-  - [a]\n", "an indicator before it has extra spaces"),
        (
            "? a\n:  b: [x]\n",
            "an indicator before it has extra spaces",
        ),
        ("k: {a: !!str ?y}\n", "an entry starts with `?`"),
        ("k: {a: \"x\n  y\"}\n", "an entry spans lines"),
        ("k: [{a: \"x\n  y\"}]\n", "an entry spans lines"),
        ("k: {a: {b: \"x\n  y\"}}\n", "an entry spans lines"),
        ("k: [&x\n  !!map {a: 1}]\n", "an entry spans lines"),
        ("k: {a: &x\n  !!map {b: 1}}\n", "an entry spans lines"),
    ] {
        assert_eq!(restyle(input, BOTH), input, "{input:?}");
        let found: Vec<String> = refusals(input, &config(BOTH))
            .into_iter()
            .map(|problem| problem.message)
            .collect();
        assert_eq!(
            found,
            [format!("cannot convert to block safely: {reason}")],
            "{input:?}"
        );
    }
    let long = format!("k: {{? {} : v}}\n", "a".repeat(1025));
    assert_eq!(restyle(&long, BOTH), long);
    assert_eq!(
        refusals(&long, &config(BOTH))[0].message,
        "cannot convert to block safely: a key is longer than 1024 characters"
    );
}

#[test]
fn a_disabled_rule_keeps_its_collections() {
    let input = "a: [b]  # ryl disable-line rule:brackets\nc: {d: e}\n";
    assert_eq!(
        restyle(input, BOTH),
        "a: [b]  # ryl disable-line rule:brackets\nc:\n  d: e\n"
    );
    let disabled = "# yamllint disable-file\na: [b]\n";
    assert_eq!(restyle(disabled, BOTH), disabled);
    assert!(refusals("# yamllint disable-file\n[a]: b\n", &config(BOTH)).is_empty());
}

#[test]
fn many_siblings_converge_in_one_run() {
    let input: String = (0..150)
        .map(|index| format!("k{index}: [a, {{b: [c]}}]\n"))
        .collect();
    let expected: String = (0..150)
        .map(|index| format!("k{index}:\n  - a\n  - b:\n      - c\n"))
        .collect();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), BOTH).unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, format!("---\n{input}")).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).arg("format").arg(&file));
    assert_eq!((code, stderr.as_str()), (0, ""));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        format!("---\n{expected}")
    );
}

#[test]
fn check_attributes_each_conversion_and_write_mode_reports_refusals() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), BOTH).unwrap();
    let file = dir.path().join("a.yaml");
    let input = "---\nk: [a]\nm: {b: 1}\nr: [c,  # note\n  d]\n";
    fs::write(&file, input).unwrap();

    let (code, _, stderr) = run(ryl(dir.path()).args(["format", "--check"]).arg(&file));
    assert_eq!(code, 1, "{stderr}");
    for (position, message, rule) in [
        ("2:4", "flow sequence would become block", "brackets"),
        ("3:4", "flow mapping would become block", "braces"),
        (
            "4:4",
            "cannot convert to block safely: it holds a comment",
            "brackets",
        ),
    ] {
        assert!(
            stderr.lines().any(|line| line.contains(position)
                && line.contains(message)
                && line.contains(rule)),
            "{position} {message}: {stderr}"
        );
    }

    let (code, _, stderr) = run(ryl(dir.path()).arg("format").arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("6:4 brackets not fixed: cannot convert to block safely"),
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\nk:\n  - a\nm:\n  b: 1\nr: [c,  # note\n    d]\n"
    );
}

const FLOW: &str = "[format]\nsequence-style = 'flow'\nmapping-style = 'flow'\n";

#[test]
fn leaf_block_collections_become_flow() {
    for (toml, input, expected) in [
        (
            FLOW,
            "x: &x v\nk:\n  - a\n  - \"b\"\n  - *x\nm:\n  a: 1\n  \"b\": c\n",
            "x: &x v\nk: [a, \"b\", *x]\nm: {a: 1, \"b\": c}\n",
        ),
        (FLOW, "k: &x !!seq\n  - a\n", "k: &x !!seq [a]\n"),
        (
            FLOW,
            "- - a\n  - -1\n- c: 1\n  d: 2\n",
            "- [a, -1]\n- {c: 1, d: 2}\n",
        ),
        (FLOW, "x: &a k\ny:\n  *a : 1\n", "x: &a k\ny: {*a : 1}\n"),
        (FLOW, "k:\n  ? a\n  : b\n", "k: {a: b}\n"),
        (
            FLOW,
            "k:\n  - &z c\n\n  - !!str 1\n",
            "k: [&z c, !!str 1]\n",
        ),
        (
            FLOW,
            "k:\n  - a\n# after\nj: 1\n",
            "k: [a]\n# after\nj: 1\n",
        ),
        (
            "[format]\nsequence-style = 'flow'\nmapping-style = 'block'\n",
            "k: {a: [b]}\nj:\n  - x: 1\n",
            "k:\n  a: [b]\nj:\n  - x: 1\n",
        ),
        (
            "[format]\nsequence-style = 'block'\nmapping-style = 'flow'\n",
            "k: [a: 1]\n",
            "k:\n  - {a: 1}\n",
        ),
    ] {
        assert_eq!(restyle(input, toml), expected, "{input:?}");
    }
}

#[test]
fn block_collections_flow_cannot_hold_stay_block() {
    for input in [
        "- a\n- b\n",
        "k:\n  - a,b\n",
        "k:\n  - c]\n",
        "k:\n  a{b: 1\n",
        "k:\n  - a\n  -\n  - b\n",
        "k:\n  a:\n  b: 2\n",
        "k:\n  - |\n    text\n",
        "k:\n  - :b\n",
        "k:\n  - ?x\n",
        "k:\n  - &x ?y\n",
        "k:\n  - a\n    b\n",
        "k:\n  - [a]\n",
        "k:  # c\n  - a\n",
        "k: # c\n  - a\n",
        "k:\n  - a  # c\n",
        "? - a\n: b\n",
        "-   a:\n      - x\n",
        "a:\n  -  - a\n     - b\n",
        "? a\n:  b:\n     - x\n",
    ] {
        assert_eq!(restyle(input, FLOW), input, "{input:?}");
    }
    let long = "key:\n  - aaaa\n  - bbbb\n";
    let narrow = format!("line-length = 16\n{FLOW}");
    assert_eq!(restyle(long, &narrow), long);
    assert_eq!(
        restyle(long, &format!("line-length = 17\n{FLOW}")),
        "key: [aaaa, bbbb]\n"
    );
}

#[test]
fn check_reports_a_block_collection_that_would_become_flow() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), FLOW).unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "---\nk:\n  - a\n").unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).args(["format", "--check"]).arg(&file));
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.lines().any(|line| line.contains("3:3")
            && line.contains("block sequence would become flow")
            && line.contains("brackets")),
        "{stderr}"
    );
}
