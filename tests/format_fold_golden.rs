//! Golden folds from the fold probe corpus; PyYAML and ruamel load every row's `expected`
//! to its input's value. Where `expected` is the corpus's folded candidate, play.yaml.com's
//! reference parser also read it as the input's events (U1 unchecked: the playground
//! mangles UTF-8). The other rows' candidates the reference parser rejected (P6, P7, P12c,
//! C1b, S2, S5c, C2b, D5), read differently (P2, P12b, P13, U2, S3, D7, D8, B2, B4, B5), or are folds
//! `ryl format` never makes: in flow collections or keys, to a column-0 root continuation
//! (P12), beside a quote (C3), with an escaped line break (D2), or to a `#`-led
//! continuation (S5, B10), which `comments-indentation` would re-indent.

use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::{FORMAT_RULE_IDS, format_str};

fn fold(input: &str, width: u16, indent: u8) -> String {
    let cfg = YamlLintConfig::from_toml_str(&format!(
        "line-length = {width}\nindent-width = {indent}\n[format]\nfold-long-lines = true\n"
    ))
    .expect("fold config parses");
    let others: Vec<&str> = FORMAT_RULE_IDS
        .into_iter()
        .filter(|rule| *rule != "line-length")
        .collect();
    format_str(input, &cfg, Path::new("golden.yaml"), &others)
}

const CASES: [(&str, &str, u16, u8, &str); 77] = [
    ("P1", "key: aaa bbb ccc\n", 12, 2, "key: aaa bbb\n  ccc\n"),
    ("P2", "key: aaa  bbb\n", 8, 2, "key: aaa  bbb\n"),
    ("P4p", "key: aaa --- bbb\n", 9, 2, "key: aaa\n  --- bbb\n"),
    ("P5", "- aaa bbb\n", 5, 2, "- aaa\n  bbb\n"),
    ("P5b", "- aaa bbb\n", 5, 1, "- aaa\n bbb\n"),
    ("P6", "key: aaa bbb\n", 8, 2, "key: aaa\n  bbb\n"),
    (
        "P7b",
        "a:\n  key: aaa bbb\n",
        10,
        1,
        "a:\n  key: aaa\n   bbb\n",
    ),
    (
        "P7c",
        "a:\n  key: aaa bbb\n",
        10,
        2,
        "a:\n  key: aaa\n    bbb\n",
    ),
    ("P8", "key: aaa b:c\n", 8, 2, "key: aaa\n  b:c\n"),
    (
        "P9",
        "key: aaa bbb # note\n",
        8,
        2,
        "key: aaa\n  bbb # note\n",
    ),
    ("P10", "k: [aaa bbb, ccc]\n", 7, 2, "k: [aaa bbb, ccc]\n"),
    ("P11", "aaa bbb: c\n", 3, 2, "aaa bbb: c\n"),
    ("P11b", "k: {aaa bbb: c}\n", 7, 2, "k: {aaa bbb: c}\n"),
    (
        "flow-value",
        "k: {x: aaa bbb}\n",
        10,
        2,
        "k: {x: aaa bbb}\n",
    ),
    ("P12", "aaa bbb\n", 3, 2, "aaa\n  bbb\n"),
    ("P12b", "aaa --- bbb\n", 3, 2, "aaa\n  ---\n  bbb\n"),
    ("P12c", "aaa ... bbb\n", 3, 2, "aaa\n  ...\n  bbb\n"),
    ("root-col-0", "aaa\nbbb ---\n", 3, 2, "aaa\nbbb\n  ---\n"),
    ("root-marker", "--- aaa bbb\n", 7, 2, "--- aaa\n  bbb\n"),
    ("P13", "key: aaa\t bbb\n", 9, 2, "key: aaa\t bbb\n"),
    (
        "P14",
        "%YAML 1.1\n---\nk: 2001-12-14 21:59:43.10 -5\n",
        16,
        2,
        "%YAML 1.1\n---\nk: 2001-12-14\n  21:59:43.10 -5\n",
    ),
    (
        "P15",
        "key: aaa\n  bbb ccc\n",
        8,
        4,
        "key: aaa\n  bbb\n  ccc\n",
    ),
    ("P16", "? aaa bbb\n: c\n", 5, 2, "? aaa bbb\n: c\n"),
    (
        "P17",
        "key: &a !!str aaa bbb\n",
        17,
        2,
        "key: &a !!str aaa\n  bbb\n",
    ),
    ("P18", "k:\n- aaa bbb\n", 5, 2, "k:\n- aaa\n  bbb\n"),
    ("P18b", "k:\n- aaa bbb\n", 5, 1, "k:\n- aaa\n bbb\n"),
    ("C1", "- key: aaa bbb\n", 10, 2, "- key: aaa\n    bbb\n"),
    (
        "dedent",
        "a:\n  b: x\nc: aaa bbb\n",
        6,
        2,
        "a:\n  b: x\nc: aaa\n  bbb\n",
    ),
    ("C1-nested", "- - aaa bbb\n", 7, 2, "- - aaa\n    bbb\n"),
    ("C5", "key: aaa bbb\r\n", 8, 2, "key: aaa\r\n  bbb\r\n"),
    ("U1", "key: 世界 世界\n", 7, 2, "key: 世界\n  世界\n"),
    ("U2", "key: aaa\u{a0}bbb\n", 8, 2, "key: aaa\u{a0}bbb\n"),
    ("literal", "k: |\n  aaa bbb\n", 4, 2, "k: |\n  aaa bbb\n"),
    ("S1", "key: 'aaa bbb'\n", 9, 2, "key: 'aaa\n  bbb'\n"),
    (
        "S1-indent-1",
        "key: 'aaa bbb'\n",
        9,
        1,
        "key: 'aaa\n  bbb'\n",
    ),
    (
        "S2",
        "a:\n  k: 'aaa bbb'\n",
        9,
        2,
        "a:\n  k: 'aaa\n    bbb'\n",
    ),
    ("S3", "key: 'aaa  bbb'\n", 9, 2, "key: 'aaa  bbb'\n"),
    ("S4", "key: 'it''s a b'\n", 11, 2, "key: 'it''s\n  a b'\n"),
    ("S5", "key: 'aaa #bbb'\n", 9, 2, "key: 'aaa #bbb'\n"),
    ("S5b", "key: 'aaa - bbb'\n", 9, 2, "key: 'aaa\n  - bbb'\n"),
    ("S5c", "'aaa --- bbb'\n", 4, 2, "'aaa\n  ---\n  bbb'\n"),
    ("S6", "'aaa bbb': c\n", 4, 2, "'aaa bbb': c\n"),
    (
        "S7",
        "key: 'aaa bbb' # n\n",
        9,
        2,
        "key: 'aaa\n  bbb' # n\n",
    ),
    ("S8", "key: 'a\\ b'\n", 8, 2, "key: 'a\\\n  b'\n"),
    ("S-seq", "- 'aaa bbb'\n", 6, 1, "- 'aaa\n  bbb'\n"),
    ("S-flow", "k: ['aaa bbb']\n", 4, 2, "k: ['aaa bbb']\n"),
    (
        "S-multiline",
        "key: 'aaa\n   bbb ccc'\n",
        8,
        2,
        "key: 'aaa\n   bbb\n   ccc'\n",
    ),
    (
        "S-multiline-seq",
        "- 'aaa\n  bbb ccc ddd'\n",
        9,
        2,
        "- 'aaa\n  bbb ccc\n  ddd'\n",
    ),
    (
        "S-multiline-seq-1",
        "- 'aaa\n bbb ccc ddd'\n",
        9,
        2,
        "- 'aaa\n bbb ccc ddd'\n",
    ),
    (
        "S-multiline-root-1",
        "'aaa\n bbb ccc ddd'\n",
        9,
        2,
        "'aaa\n bbb ccc ddd'\n",
    ),
    (
        "D-multiline-seq-1",
        "- \"aaa\\\n bbb ccc ddd\"\n",
        9,
        2,
        "- \"aaa\\\n bbb ccc ddd\"\n",
    ),
    ("D1", "key: \"aaa bbb\"\n", 9, 2, "key: \"aaa\n  bbb\"\n"),
    ("D2", "key: \"aaabbb\"\n", 9, 2, "key: \"aaabbb\"\n"),
    ("D5", "key: \"a\\u00e9b\"\n", 9, 2, "key: \"a\\u00e9b\"\n"),
    ("D6", "key: \"a\\t b\"\n", 9, 2, "key: \"a\\t\n  b\"\n"),
    ("D7", "key: \"a\\ b\"\n", 8, 2, "key: \"a\\ b\"\n"),
    (
        "D7-even",
        "key: \"a\\\\ b\"\n",
        9,
        2,
        "key: \"a\\\\\n  b\"\n",
    ),
    ("D8", "key: \"aaa  bbb\"\n", 9, 2, "key: \"aaa  bbb\"\n"),
    ("D9", "\"aaa bbb\": c\n", 4, 2, "\"aaa bbb\": c\n"),
    ("C2", "- key: 'aaa bbb'\n", 11, 2, "- key: 'aaa\n    bbb'\n"),
    ("C3", "key: 'aaa '\n", 9, 2, "key: 'aaa '\n"),
    ("C4", "key: \"a \\tb\"\n", 7, 2, "key: \"a\n  \\tb\"\n"),
    ("B1", "k: >\n  aaa bbb\n", 5, 2, "k: >\n  aaa\n  bbb\n"),
    (
        "B2",
        "k: >\n  x\n    aaa bbb\n",
        5,
        2,
        "k: >\n  x\n    aaa bbb\n",
    ),
    (
        "B3",
        "k: >\n  aaa bbb\n    ind\n",
        5,
        2,
        "k: >\n  aaa\n  bbb\n    ind\n",
    ),
    ("B4", "k: >\n  aaa  bbb\n", 5, 2, "k: >\n  aaa  bbb\n"),
    (
        "B6",
        "k: >2\n   lead\n  aaa bbb\n",
        5,
        2,
        "k: >2\n   lead\n  aaa\n  bbb\n",
    ),
    ("B7", "k: >-\n  aaa bbb\n", 5, 2, "k: >-\n  aaa\n  bbb\n"),
    (
        "B8",
        "k: >+\n  aaa bbb\n\n",
        5,
        2,
        "k: >+\n  aaa\n  bbb\n\n",
    ),
    ("B9", "k: >\n\n  aaa bbb\n", 5, 2, "k: >\n\n  aaa\n  bbb\n"),
    ("B10", "k: >\n  aaa #bbb\n", 5, 2, "k: >\n  aaa #bbb\n"),
    (
        "B11",
        "k: >\n  aaa bbb\n\n  ccc\n",
        5,
        2,
        "k: >\n  aaa\n  bbb\n\n  ccc\n",
    ),
    (
        "C6",
        "- key: >\n    aaa bbb\n",
        8,
        2,
        "- key: >\n    aaa\n    bbb\n",
    ),
    (
        "B-tab",
        "k: >\n  normal\n  \taaa bbb ccc\n  end\n",
        10,
        2,
        "k: >\n  normal\n  \taaa bbb ccc\n  end\n",
    ),
    (
        "B-root-0",
        "--- >\naaa --- bbb\n",
        4,
        2,
        "--- >\naaa --- bbb\n",
    ),
    (
        "B-root-2",
        "--- >\n  aaa bbb\n",
        5,
        2,
        "--- >\n  aaa\n  bbb\n",
    ),
    (
        "B-header",
        "k: >-  # a b c\n  x\n",
        4,
        2,
        "k: >-  # a b c\n  x\n",
    ),
];

const CONTINUATION_WORDS: [&str; 17] = [
    "- bbb", "-bbb", "? bbb", ":bbb", "&bbb", "*bbb", "!bbb", "[bbb]", "{bbb}",
    "| bbb", "> bbb", "'bbb'", "\"bbb\"", "%bbb", "@bbb", "`bbb`", ",bbb",
];

#[test]
fn folds_match_the_probe_corpus() {
    for (id, input, width, indent, expected) in CASES {
        assert_eq!(
            fold(input, width, indent),
            expected,
            "{id} at width {width}, indent {indent}"
        );
    }
}

#[test]
fn a_continuation_may_start_with_any_indicator() {
    for word in CONTINUATION_WORDS {
        assert_eq!(
            fold(&format!("key: aaa {word}\n"), 8, 2),
            format!("key: aaa\n  {word}\n"),
            "continuation {word:?}"
        );
    }
}

#[test]
fn a_break_must_shorten_the_line_counted_in_chars_not_bytes() {
    let input = format!("{} b\n", "界".repeat(100));
    assert_eq!(
        fold(&input, 10, 100),
        format!("{}\n{}b\n", "界".repeat(100), " ".repeat(100))
    );
    assert_eq!(fold(&input, 10, 101), input);
}
