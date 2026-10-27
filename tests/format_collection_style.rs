//! `[format] sequence-style` / `mapping-style` from the collection-style probe corpus.
//! PyYAML, ruamel, the `yaml` npm package and the parser load each row's `expected` to
//! its input's events, collection style aside; every input left unchanged is one where
//! one of those parsers reads the other style differently, or the rewrite would lose a
//! comment or a line break or, under `flow`, outgrow `line-length`.

use std::fs;
use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::{FORMAT_RULE_IDS, format_str, refusals};
use tempfile::tempdir;

use crate::common;
use common::cli::{run, ryl};

const BOTH: &str = "[format]\nsequence-style = 'block'\nmapping-style = 'block'\n";
const SEQUENCES: &str = "[format]\nsequence-style = 'block'\n";
const MAPPINGS: &str = "[format]\nmapping-style = 'block'\n";

#[test]
fn converted_children_keep_the_original_trailing_comment_alignment() {
    let input = "a: {b: c}\n # d\n";
    let cfg = config(MAPPINGS);
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "a:\n  b: c\n# d\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn flow_reconsideration_uses_split_dash_spacing() {
    let input = "root:\n  records:\n    -    name: 'alpha'\n         other: ok\n";
    let cfg = config(
        "indent-width=1\nline-length=27\n[format]\nmapping-style='flow'\nindent-sequences=false\ndash-on-own-line=true\n",
    );
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "root:\n records:\n - {name: alpha, other: ok}\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn plain_marker_keys_keep_the_safe_collection_context() {
    let cfg = config(MAPPINGS);
    for (input, expected) in [
        ("{\"--- x\": \"a\n b\"}\n", "{--- x: a b}\n"),
        ("{\"... x\": \"a\n b\"}\n", "{... x: a b}\n"),
        ("{--- x: a}\n", "{--- x: a}\n"),
        ("{\"--- x\": \"a\"}\n", "'--- x': a\n"),
        ("{\"---\": \"a\n b\"}\n", "---: a b\n"),
        ("{\"---x\": \"a\n b\"}\n", "---x: a b\n"),
        ("outer: {\"--- x\": \"a\n b\"}\n", "outer:\n  --- x: a b\n"),
        ("{&k \"--- x\": \"a\n b\"}\n", "&k --- x: a b\n"),
        ("{\"key\": \"--- x\"}\n", "key: --- x\n"),
    ] {
        let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
        assert_eq!(output, expected, "{input:?}");
        crate::property::reference::check_strict_events(input, &output).unwrap();
        assert_eq!(
            format_str(&output, &cfg, Path::new("case.yaml"), &[]),
            output
        );
    }
}

#[test]
fn flow_conversion_reconsiders_quote_selected_widths() {
    let cfg = config("line-length = 17\n[format]\nsequence-style = 'flow'\n");
    let input = "key:\n  - \"aaaa\"\n  - \"bbbb\"\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "key: [aaaa, bbbb]\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn flow_conversion_keeps_context_dependent_quote_eligibility() {
    let input = "key:\n  - \"a,b\"\n  - \"c,d\"\n";
    for (width, expected) in [
        (19, "key: ['a,b', 'c,d']\n"),
        (18, "key:\n  - a,b\n  - c,d\n"),
    ] {
        let cfg = config(&format!(
            "line-length = {width}\n[format]\nsequence-style = 'flow'\n"
        ));
        let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
        assert_eq!(output, expected);
        crate::property::reference::check_strict_events(input, &output).unwrap();
        assert_eq!(
            format_str(&output, &cfg, Path::new("case.yaml"), &[]),
            output
        );
    }
}

#[test]
fn converted_children_follow_the_moved_quote_directive() {
    let cfg = config(SEQUENCES);
    let input = "key: [\"aaaa\", \"bbbb\"]  # ryl disable-line rule:quoted-strings\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "key:  # ryl disable-line rule:quoted-strings\n  - aaaa\n  - bbbb\n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converting_a_child_keeps_the_parent_nonleaf() {
    let cfg = config("[format]\nsequence-style = 'flow'\n");
    let input = "key:\n  - - a\n    - b\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "key:\n  - [a, b]\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn implicit_mappings_settle_block_quotes_before_flow_eligibility() {
    for (input, mapping, expected) in [
        ("key: [a: \"b\n  c\"]\n", "flow", "key:\n  - {a: b c}\n"),
        ("key: [a: \"b\n  c\"]\n", "preserve", "key:\n  - a: b c\n"),
        ("key: [a: \"b,c\"]\n", "flow", "key:\n  - a: b,c\n"),
        (
            "key: [a: \"b\n  c\"]\nother: [d]\n",
            "flow",
            "key:\n  - {a: b c}\nother:\n  - d\n",
        ),
        (
            "key: [a: \"b,\n  c\"]\nother: [d]\n",
            "flow",
            "key: [a: 'b,\n  c']\nother:\n  - d\n",
        ),
        (
            "key:   [a: \"b,c\"]\nother: [d]\n",
            "flow",
            "key:\n  - a: b,c\nother:\n  - d\n",
        ),
    ] {
        let cfg = config(&format!(
            "[format]\nsequence-style = 'block'\nmapping-style = '{mapping}'\n"
        ));
        let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
        assert_eq!(output, expected);
        crate::property::reference::check_strict_events(input, &output).unwrap();
        assert_eq!(
            format_str(&output, &cfg, Path::new("case.yaml"), &[]),
            output
        );
    }
}

#[test]
fn indentless_sequence_conversion_does_not_keep_its_first_dash() {
    let cfg = config("[format]\nsequence-style = 'flow'\nindent-sequences = false\n");
    let input = "items:\n- café\n- three\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "items: [café, three]\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
}

#[test]
fn flow_conversion_measures_width_at_the_target_indent() {
    let cfg = config(
        "indent-width = 4\nline-length = 28\n[format]\nmapping-style = 'flow'\n",
    );
    for input in [
        "records:\n  - name: alpha\n    other: ok\n",
        "records:\n   - name: alpha\n     other: ok\n",
    ] {
        let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
        assert_eq!(output, "records:\n    - name: alpha\n      other: ok\n");
        crate::property::reference::check_strict_events(input, &output).unwrap();
    }
}

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
fn flow_conversion_settles_default_indent_around_explicit_keys() {
    for (toml, input, expected) in [
        (
            "[format]\nsequence-style = 'flow'\ndash-on-own-line = true\n",
            "a:\r\n   - a\r\n?\r\n: - ?\r\n",
            "a: [a]\n?\n: -\n    ?\n",
        ),
        (
            "[format]\nsequence-style = 'flow'\n",
            "a:\n   - a\n?\n: >+\n a",
            "a: [a]\n?\n: >+\n    a\n",
        ),
    ] {
        let cfg = config(toml);
        let once = format_str(input, &cfg, Path::new("example.yaml"), &[]);
        assert_eq!(once, expected, "{input:?}");
        assert_eq!(
            format_str(&once, &cfg, Path::new("example.yaml"), &[]),
            once,
            "{input:?}"
        );
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
        ("k: [a, # café\n b]\n---\n[", "it holds a comment"),
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
fn flow_conversion_waits_for_document_reindent() {
    let cases = [
        (
            "sequence, flush",
            "[format]\nsequence-style = 'block'\nindent-sequences = false\n[lint.rules.indentation]\nindent-sequences = false\n",
            "a: [a]\nb: >\n   x\n  #\n",
        ),
        (
            "sequence, indented",
            "[format]\nsequence-style = 'block'\nindent-sequences = true\n",
            "a: [a]\nb: >\n   x\n  #\n",
        ),
        (
            "mapping, flush",
            "[format]\nmapping-style = 'block'\nindent-sequences = false\n",
            "a: {b: c}\nd: >\n   x\n  #\n",
        ),
        (
            "mapping, indented",
            "[format]\nmapping-style = 'block'\nindent-sequences = true\n",
            "a: {b: c}\nd: >\n   x\n  #\n",
        ),
    ];
    for (name, toml, input) in cases {
        let cfg = config(toml);
        assert_eq!(
            format_str(input, &cfg, Path::new("case.yaml"), &[]),
            input,
            "{name}"
        );
        assert!(
            refusals(input, &cfg).iter().any(|problem| problem
                .message
                .contains("document cannot be re-indented")),
            "{name} should report the collection refusal"
        );
    }
}

#[test]
fn block_to_flow_survives_unrelated_reindent_refusal() {
    let input = "a:\n  - b\nbad: >\n   x\n  #\n";
    let expected = "a: [b]\nbad: >\n   x\n  #\n";
    let toml = "[format]\nsequence-style = 'flow'\n";
    assert_eq!(
        format_str(input, &config(toml), Path::new("case.yaml"), &[]),
        expected
    );

    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), toml).unwrap();
    let file = dir.path().join("case.yaml");
    fs::write(&file, input).unwrap();
    let (code, _, stderr) = run(ryl(dir.path()).args(["format", "--check"]).arg(&file));
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("block sequence would become flow"),
        "{stderr}"
    );
}

#[test]
fn flow_conversion_waits_for_refused_dash_layout() {
    let input = "items: [{a: b}]\nbad: >\n   x\n  #\n";
    let cfg = config(
        "[format]\nsequence-style = 'block'\nmapping-style = 'block'\nindent-sequences = true\ndash-on-own-line = true\n",
    );
    assert_eq!(format_str(input, &cfg, Path::new("case.yaml"), &[]), input);
    assert!(
        refusals(input, &cfg)
            .iter()
            .any(|problem| problem.message.contains("document cannot be re-indented"))
    );
    assert_eq!(
        format_str("items: [{a: b}]\n", &cfg, Path::new("case.yaml"), &[]),
        "items:\n  -\n    a: b\n"
    );
}

#[test]
fn flow_conversions_in_same_refused_document_stay_flow() {
    let input = "a: [a]\nm: {b: c}\nbad: >\n   x\n  #\n";
    let toml = "[format]\nsequence-style = 'block'\nmapping-style = 'block'\nindent-sequences = false\n";
    let cfg = config(toml);
    assert_eq!(format_str(input, &cfg, Path::new("case.yaml"), &[]), input);
    let found = refusals(input, &cfg);
    assert!(found.iter().any(|problem| problem.rule == Some("brackets")
        && problem.message.contains("document cannot be re-indented")));
    assert!(found.iter().any(|problem| problem.rule == Some("braces")
        && problem.message.contains("document cannot be re-indented")));
}

#[test]
fn flow_conversion_handles_each_reindent_refusal_cause() {
    use ryl::rules::indentation::{
        Cause, Config, IndentSequencesSetting, SpacesSetting, reindent,
    };

    let target = Config::new(
        SpacesSetting::Fixed(2),
        IndentSequencesSetting::False,
        false,
    );
    let cfg = config(
        "indent-width = 2\n[format]\nsequence-style = 'block'\nindent-sequences = false\n",
    );
    for (cause, suffix) in [
        (Cause::Changed, "b: >\n   x\n  #\n"),
        (Cause::Tab, "b:\n  c: d\n \t\n"),
        (
            Cause::Disabled,
            "b: c  # yamllint disable-line rule:indentation\n",
        ),
        (Cause::Unfollowable, ": value\n"),
    ] {
        let candidate = format!("a:\n  - a\n{suffix}");
        assert_eq!(reindent(&candidate, &target).refused[0].cause, cause);
        let input = format!("a: [a]\n{suffix}");
        let formatted = format_str(&input, &cfg, Path::new("case.yaml"), &[]);
        assert_eq!(
            formatted.contains("a: [a]"),
            cause != Cause::Tab,
            "{cause:?}: {formatted:?}"
        );
    }
}

#[test]
fn exact_repro_keeps_the_agreeing_indentation_config_clean() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    let config = dir.path().join("config.toml");
    let input = "a: [a]\nb: >\n   x\n  #\n";
    fs::write(&file, input).unwrap();
    fs::write(
        &config,
        "[format]\nsequence-style = 'block'\nindent-sequences = false\n[lint.rules.indentation]\nindent-sequences = false\n",
    )
    .unwrap();

    let check = || {
        run(ryl(dir.path())
            .args(["check", "-c"])
            .arg(&config)
            .arg(&file))
    };
    assert_eq!(check().0, 0);
    let (code, _, stderr) = run(ryl(dir.path())
        .args(["format", "-c"])
        .arg(&config)
        .arg(&file));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stderr.contains("brackets not fixed: cannot convert to block safely"),
        "{stderr}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), input);
    assert_eq!(check().0, 0);
}

#[test]
fn a_reindent_refusal_does_not_block_another_document() {
    let input =
        "---\na: [a]\nb: c  # yamllint disable-line rule:indentation\n---\nc: [d]\n";
    let expected =
        "---\na: [a]\nb: c  # yamllint disable-line rule:indentation\n---\nc:\n- d\n";
    let cfg = config(
        "indent-width = 2\n[format]\nsequence-style = 'block'\nindent-sequences = false\n",
    );
    assert_eq!(
        format_str(input, &cfg, Path::new("case.yaml"), &[]),
        expected
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

#[test]
fn nested_mapping_values_keep_their_collection_properties() {
    assert_eq!(
        restyle("k: {a: &x !!seq [b, c]}\n", BOTH),
        "k:\n  a: &x !!seq\n    - b\n    - c\n"
    );
}

#[test]
fn comma_edit_refreshes_collection_and_quote_events() {
    let input = "items: [one,'two']\n";
    assert_eq!(
        format_str(input, &config(SEQUENCES), Path::new("items.yaml"), &[]),
        "items:\n  - one\n  - two\n"
    );
}

#[test]
fn bom_converted_children() {
    let input = "﻿root: [one, two]\n";
    let cfg = config("[format]\nsequence-style=\"block\"\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "﻿root:\n  - one\n  - two\n");
    crate::property::reference::check_strict_events(
        input.trim_start_matches('\u{feff}'),
        output.trim_start_matches('\u{feff}'),
    )
    .unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_sibling_preserves_join() {
    let input = "first: [a,b]\nrecords:\n  -\n    key: value\n";
    let cfg = config("[format]\nsequence-style=\"block\"\ndash-on-own-line=false\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "first:\n  - a\n  - b\nrecords:\n  - key: value\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_sibling_retains_flow_tokens() {
    let input = "first: [a,b]\nother: {key: [x,y]}\n";
    let cfg = config("[format]\nsequence-style=\"block\"\nfold-long-lines=true\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "first:\n  - a\n  - b\nother: {key: [x, y]}\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_empty_entries() {
    let input = "key: {a: , b: }\n";
    let cfg = config("[format]\nmapping-style=\"block\"\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "key:\n  a:\n  b:\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_sibling_joins_touching_quotes() {
    let input = "first: [a,b]\nother: [\"one\n two\", \"three\n four\"]\n";
    let cfg = config("[format]\nsequence-style=\"block\"\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "first:\n  - a\n  - b\nother:\n  - one two\n  - three four\n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn reconsideration_keeps_disabled_trailing_spaces() {
    let input = "key:\n - \"aaaa\"  \n - \"bbbb\"\n# ryl disable rule:trailing-spaces\nother: value  \n";
    let cfg = config("line-length=17\n[format]\nsequence-style=\"flow\"\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "key: [aaaa, bbbb]\n# ryl disable rule:trailing-spaces\nother: value  \n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_sibling_keeps_above_line_quote_directives() {
    let input = "first: [a,b]\n# ryl disable rule:quoted-strings\nother: \"value\"\n";
    let cfg = config(SEQUENCES);
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "first:\n  - a\n  - b\n# ryl disable rule:quoted-strings\nother: \"value\"\n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_mapping_joins_its_explicit_value_dash() {
    let input = "? a\n:  -   { é :  [ \"é\tvalue\" ,y ] }\n";
    let cfg = config(
        "[format]\nquote-style='preserve'\nsequence-style='block'\nmapping-style='block'\n",
    );
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "? a\n: - é:\n      - \"é\tvalue\"\n      - y\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn moved_comment_uses_the_wider_selected_attachment_gap() {
    let input = "first: [a,b] #keep\n";
    let cfg = config("[format]\nsequence-style='block'\ncomment-spacing=3\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "first:   # keep\n  - a\n  - b\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn tight_join_quotes_feed_collection_widths() {
    let input = "-\n a: 'one\n  two'\n b:\n - \"aaaa\"\n - \"bbbb\"\n";
    let cfg = config("line-length=20\n[format]\nsequence-style='flow'\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "- a: one two\n  b: [aaaa, bbbb]\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_sibling_keeps_the_other_documents_refused_dash_shape() {
    let input = "first: [a,b]\n---\n# ryl disable rule:indentation\nrecords:\n -   key: value\n";
    let cfg = config("[format]\nsequence-style='block'\ndash-on-own-line=true\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "first:\n  - a\n  - b\n---\n# ryl disable rule:indentation\nrecords:\n - key: value\n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn joined_explicit_key_closes_its_gap_when_colons_are_skipped() {
    let input = "-\n  ?   a\n  : b\n";
    let cfg = config("[format]\nmapping-style='flow'\ndash-on-own-line=false\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &["colons"]);
    assert_eq!(output, "- {a: b}\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &["colons"]),
        output
    );
}

#[test]
fn tagged_empty_key_uses_the_selected_colon_gap() {
    let input = "!local :  {\"a\":\"value\"} \t#note\n";
    let cfg = config(MAPPINGS);
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "!local :  # note\n  a: value\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn split_mapping_reconsideration_merges_its_selected_gap() {
    let input = "-    a: b\n";
    let cfg = config(
        "line-length=8\n[format]\nmapping-style='flow'\ndash-on-own-line=true\n",
    );
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "- {a: b}\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_sibling_maps_a_kept_dash_after_an_accepted_split() {
    let input = "first: [a,b]\nrecords:\n  - key: value\n  - kept: value # keep\n";
    let cfg = config("[format]\nsequence-style='block'\ndash-on-own-line=true\n");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "first:\n  - a\n  - b\nrecords:\n  -\n    key: value\n  - kept: value  # keep\n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_sibling_folds_after_splitting_a_mapping_dash() {
    let input = "first: [a,b]\nrecords:\n  - key: one two three four five six seven eight nine ten\n";
    let cfg = config(
        "line-length=24\n[format]\nsequence-style='block'\ndash-on-own-line=true\nfold-long-lines=true\n",
    );
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "first:\n  - a\n  - b\nrecords:\n  -\n    key: one two three\n      four five six\n      seven eight nine\n      ten\n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn converted_sibling_reconsiders_flow_width_after_splitting_a_dash() {
    let input = "first: [a,b]\nrecords:\n  - key: one two three four five six seven eight nine ten\nother:\n  longkey: \"one\"\n  nextkey: \"two\"\n";
    let cfg = config(
        "line-length=36\n[format]\nsequence-style='block'\nmapping-style='flow'\ndash-on-own-line=true\nfold-long-lines=true\n",
    );
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "first:\n  - a\n  - b\nrecords:\n  -\n    key: one two three four five six\n      seven eight nine ten\nother: {longkey: one, nextkey: two}\n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn reconsidered_mapping_counts_the_explicit_value_gap_once() {
    let cfg = config(
        "[format]\nquote-style='double'\nsequence-style='flow'\nmapping-style='block'\nindent-sequences=false\ndash-on-own-line=false\ndocument-start='add'\n",
    );
    for (input, expected) in [
        (
            "? a\n:  -   {k: [x]}\n   - two\n",
            "---\n? a\n: - k: [x]\n  - two\n",
        ),
        (
            "? a\n:  -   {k: [x]}\n   - \"one\n        two\"\n",
            "---\n? a\n: - k: [x]\n  - one two\n",
        ),
    ] {
        let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
        assert_eq!(output, expected);
        crate::property::reference::check_strict_events(input, &output).unwrap();
        assert_eq!(
            format_str(&output, &cfg, Path::new("case.yaml"), &[]),
            output
        );
        assert!(ryl::format::conflicts(&cfg).is_empty());
    }
}
