//! `[format] sequence-expand` / `mapping-expand`, and `*-style = "flow"` turning an
//! overflowing flow collection into block. Each case is checked against the reference
//! parser's events and for a second run leaving it unchanged.

use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::{format_str, refusals};

const AUTO: &str = "[format]\nsequence-expand = 'auto'\nmapping-expand = 'auto'\n";

#[test]
fn standalone_value_comments_survive_collapsed_quotes() {
    for scalar in ["\"a\n    b\"", "a b"] {
        for expand in ["always", "never"] {
            formats(
                &format!("k:\n  [{scalar}] # c\n"),
                &format!("[format]\nsequence-expand='{expand}'\n"),
                "k:\n  [a b]  # c\n",
            );
        }
    }
}

#[test]
fn removed_inline_document_marker_does_not_defer_expansion() {
    formats(
        "--- {k: v}\n",
        "[format]\ndocument-start='remove'\nmapping-expand='always'\n",
        "{\n  k: v,\n}\n",
    );
    formats(
        "--- &col [\"a\n    b\"] # c\n",
        "indent-width=8\nline-length=12\n[format]\ndocument-start='remove'\nsequence-style='flow'\nsequence-expand='always'\n",
        "&col [  # c\n        a b,\n]\n",
    );
}

#[test]
fn marker_removal_projection_obeys_rule_directives() {
    for (rule, marker) in [("quoted-strings", ""), ("document-start", "--- ")] {
        formats(
            &format!("--- {{k: v}} # ryl disable-line rule:{rule}\n"),
            "[format]\ndocument-start='remove'\nmapping-expand='always'\n",
            &format!("{marker}{{  # ryl disable-line rule:{rule}\n  k: v,\n}}\n"),
        );
    }
}

#[test]
fn standalone_anchor_uses_final_indent_for_flow_eligibility() {
    for expand in ["preserve", "never", "always"] {
        formats(
            "-\n  &col {k: v}\n",
            &format!(
                "line-length=12\nindent-width=1\n[format]\nmapping-style='flow'\nmapping-expand='{expand}'\n"
            ),
            if expand == "always" {
                "-\n &col {\n  k: v,\n }\n"
            } else {
                "-\n &col {k: v}\n"
            },
        );
    }
}

#[test]
fn added_document_start_keeps_root_collection_geometry_stable() {
    for (input, expand) in [("[a]\n", "sequence"), ("{k: v}\n", "mapping")] {
        formats(
            input,
            &format!("[format]\ndocument-start = 'add'\n{expand}-expand = 'auto'\n"),
            &format!("---\n{input}"),
        );
    }
    formats(
        "[abc]\n",
        "line-length = 5\n[format]\ndocument-start = 'add'\nsequence-style = 'flow'\n",
        "---\n[abc]\n",
    );
}

#[test]
fn collapsed_multiline_quotes_allow_commented_collection_expansion() {
    formats(
        "{k: \"a\n b\"} # c\n",
        "[format]\nmapping-expand = 'always'\n",
        "{  # c\n  k: a b,\n}\n",
    );
}

#[test]
fn commented_collection_expands_below_a_compact_mappings_key() {
    for scalar in ["\"a\n   b\"", "a b"] {
        formats(
            &format!("- k: [{scalar}] # c\n"),
            "[format]\ndash-on-own-line = true\nsequence-expand = 'always'\n",
            "- k:  # c\n    [\n      a b,\n    ]\n",
        );
    }
}

#[test]
fn explicit_key_flow_overflow_uses_the_final_sequence_indent() {
    formats(
        "? a\n: [[a, b], c]\n",
        "indent-width=8\nline-length=10\n[format]\nsequence-style='flow'\nindent-sequences=false\n",
        "? a\n:\n        - - a\n          - b\n        - c\n",
    );
    formats(
        "root:\n- ? a\n  : [[a, b], c]\n",
        "line-length=10\n[format]\nsequence-style='flow'\nindent-sequences=false\n",
        "root:\n- ? a\n  :\n    - - a\n      - b\n    - c\n",
    );
}

#[test]
fn explicit_key_geometry_tracks_empty_complex_and_property_keys() {
    let settings = "indent-width=8\nline-length=10\n[format]\nsequence-style='flow'\nindent-sequences=false\n";
    let cfg = YamlLintConfig::from_toml_str(settings).unwrap();
    for input in [
        "? [a, b]\n: [[a, b], c]\n",
        "? [a, b] : [[a, b], c]\n",
        "? inner: value\n: [[a, b], c]\n",
        "? - a\n  - b\n: [[a, b], c]\n",
        "? &key a\n: [[a, b], c]\n",
        "? # key\n  a\n: [[a, b], c]\n",
        ": zero\n? a\n: [[a, b], c]\n",
        "flow: {: zero, a: one}\n? a\n: [[a, b], c]\n",
    ] {
        let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
        crate::property::reference::check_strict_events(input, &output).unwrap();
        assert_eq!(
            format_str(&output, &cfg, Path::new("case.yaml"), &[]),
            output,
            "{input:?}"
        );
    }
    for input in [
        "a: [[a, b], c]\n",
        "previous: abc ?\n: [[a, b], c]\n",
        "{? a: [[a, b], c]}\n",
    ] {
        let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
        crate::property::reference::check_strict_events(input, &output).unwrap();
        assert!(!output.contains("- - a"), "{input:?}: {output:?}");
        assert_eq!(
            format_str(&output, &cfg, Path::new("case.yaml"), &[]),
            output,
            "{input:?}"
        );
    }
}

fn formats(input: &str, toml: &str, expected: &str) {
    let cfg = YamlLintConfig::from_toml_str(toml).expect("expand config parses");
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, expected, "input {input:?} under {toml:?}");
    crate::property::reference::check_strict_events(input, &output).unwrap();
    crate::property::representation::check_annotations(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output,
        "second run of {output:?}"
    );
}

#[test]
fn auto_writes_prettiers_layout_where_a_line_overflows() {
    formats(
        "items: [first-item-with-long-name, second-item-with-long-name, third, fourth, fx]\n\
         short: [\n  a, b,\n  ]\n\
         top:\n  nested: {alpha-key-long: 1, beta-key-long: [x-value-long-one, y-value-long-two, z-value-long-3, w-value-long-four]}\n\
         anch: &a !!seq [first-item-with-long-name, second-item-with-long-name, third, fourth]\n\
         pairs: [a: 1, bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb: 2, cccccccccccccccccccccccccccc: 3]\n\
         empty-val: {aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb: }\n\
         al: [*a, *a]\n",
        AUTO,
        "items:\n  [first-item-with-long-name, second-item-with-long-name, third, fourth, fx]\n\
         short: [a, b]\n\
         top:\n  nested:\n    {\n      alpha-key-long: 1,\n      beta-key-long:\n        \
         [x-value-long-one, y-value-long-two, z-value-long-3, w-value-long-four],\n    }\n\
         anch:\n  &a !!seq [\n    first-item-with-long-name,\n    second-item-with-long-name,\n    third,\n    fourth,\n  ]\n\
         pairs:\n  [\n    a: 1,\n    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb: 2,\n    cccccccccccccccccccccccccccc: 3,\n  ]\n\
         empty-val:\n  {\n    aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa,\n    bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:,\n  }\n\
         al: [*a, *a]\n",
    );
}

#[test]
fn auto_moves_a_value_that_fits_alone_to_the_next_line() {
    let auto = |width| format!("line-length = {width}\n{AUTO}");
    let input = "k: [aaaa, bbbb]\n";
    formats(input, &auto(15), input);
    formats(input, &auto(14), "k:\n  [aaaa, bbbb]\n");
    formats(input, &auto(13), "k:\n  [\n    aaaa,\n    bbbb,\n  ]\n");
    formats("k:\n  [aaaa, bbbb]\n", &auto(15), input);
}

#[test]
fn a_trailing_comment_stays_on_the_opening_bracket_line() {
    let auto = |width| format!("line-length = {width}\n{AUTO}");
    formats(
        "k: [aaaa, bbbb]  # c\ns:\n  - {aaaa: 1, bbbb: 2}  # d\n",
        &auto(14),
        "k: [  # c\n  aaaa,\n  bbbb,\n ]\ns:\n  - {  # d\n    aaaa: 1,\n    bbbb: 2,\n   }\n",
    );
    formats(
        "top:\n  k: [aaaa, bbbb]  # c\n",
        &auto(16),
        "top:\n  k: [  # c\n    aaaa,\n    bbbb,\n   ]\n",
    );
}

#[test]
fn a_spread_collection_with_a_comment_after_its_bracket_keeps_its_lines() {
    formats(
        "a: [true,\n  tail,\n  ]  # s\n",
        &format!("line-length = 80\n{AUTO}"),
        "a: [true,\n    tail,\n ]  # s\n",
    );
}

#[test]
fn a_commented_value_formatting_joins_onto_a_dash_line_stays_on_one_line() {
    formats(
        "-\n  k: [aaaa, bbbb]  # c\n",
        &format!("line-length = 12\n{AUTO}"),
        "- k: [aaaa, bbbb]  # c\n",
    );
}

#[test]
fn a_commented_value_after_a_dash_stays_on_one_line() {
    let input = "- k: [aaaa, bbbb]  # c\n";
    formats(input, &format!("line-length = 12\n{AUTO}"), input);
}

#[test]
fn a_collection_in_a_key_value_entry_of_a_flow_sequence_is_respelled_too() {
    formats(
        "k: [a: [x, y]]\n",
        "[format]\nsequence-expand = 'always'\n",
        "k:\n  [\n    a:\n      [\n        x,\n        y,\n      ],\n  ]\n",
    );
}

#[test]
fn a_trailing_ytt_directive_keeps_its_collection_on_one_line() {
    let input = "k: [aaaa, bbbb]  #@ overlay/match by=\"name\"\n";
    formats(input, "[format]\nsequence-expand = 'always'\n", input);
}

#[test]
fn a_trailing_comment_is_left_out_of_the_width() {
    let input = "k: [aaaa, bbbb]  # a long trailing comment\n";
    formats(input, &format!("line-length = 15\n{AUTO}"), input);
}

#[test]
fn auto_moves_a_nested_value_counting_its_trailing_comma() {
    let config = |width| {
        format!(
            "line-length = {width}\n[format]\nsequence-expand = 'auto'\n\
             mapping-expand = 'always'\n"
        )
    };
    let input = "m: {k: [aaaa, bbbb]}\n";
    formats(input, &config(20), "m:\n  {\n    k: [aaaa, bbbb],\n  }\n");
    formats(
        input,
        &config(19),
        "m:\n  {\n    k:\n      [aaaa, bbbb],\n  }\n",
    );
    formats(
        input,
        &config(18),
        "m:\n  {\n    k:\n      [\n        aaaa,\n        bbbb,\n      ],\n  }\n",
    );
}

#[test]
fn auto_expands_a_root_and_a_sequence_entry_on_their_own_lines() {
    formats(
        "[first-item-with-long-name, second-item-with-long-name, third, fourth, fifth-xyz]\n",
        AUTO,
        "[\n  first-item-with-long-name,\n  second-item-with-long-name,\n  third,\n  fourth,\n  fifth-xyz,\n]\n",
    );
    formats(
        "- [alpha, beta, gamma]\n",
        "line-length = 12\n[format]\nsequence-expand = 'auto'\n",
        "- [\n  alpha,\n  beta,\n  gamma,\n ]\n",
    );
}

#[test]
fn a_spread_child_spreads_its_parents() {
    formats(
        "s: [a, b]\nm: {a: 1,\n  b: [x]}\nmm: {a: {b: 1,\n  c: 2}}\ne: []\nq: [\"x\", 'y']\n",
        "[format]\nsequence-expand = 'always'\nmapping-expand = 'never'\nbrace-spacing = true\n",
        "s:\n  [\n    a,\n    b,\n  ]\nm:\n  {\n    a: 1,\n    b:\n      [\n        x,\n      ],\n  }\n\
         mm: { a: { b: 1, c: 2 } }\ne: []\nq:\n  [\n    x,\n    'y',\n  ]\n",
    );
}

#[test]
fn a_collection_holding_a_comment_stays_with_a_notice() {
    let input = "c: [a, b]  # keep\nd: {a: 1,  # inner\n  b: 2}\n";
    let toml = "[format]\nsequence-expand = 'always'\nmapping-expand = 'never'\n";
    formats(
        input,
        toml,
        "c: [  # keep\n  a,\n  b,\n ]\nd: {a: 1,  # inner\n    b: 2}\n",
    );
    let cfg = YamlLintConfig::from_toml_str(toml).unwrap();
    let notices: Vec<_> = refusals(input, &cfg)
        .into_iter()
        .map(|problem| (problem.line, problem.message))
        .collect();
    let message = "cannot change line breaks safely: a comment is in the way";
    assert_eq!(notices, [(2, message.to_owned())], "{notices:?}");
}

#[test]
fn an_entry_after_a_second_dash_stays_on_one_line() {
    formats(
        "'it''s':\r- - [a]\r",
        "[format]\nsequence-expand = 'always'\n",
        "it's:\n  - - [a]\n",
    );
}

#[test]
fn a_trailing_comment_after_a_spread_collection_keeps_its_column() {
    formats(
        " #a\na:\n  - &a0 ! {  a73  : \"it's \\\"x\\\": y\"  }\n #a\ncollection_0:\n  - &a1 ! {  a73  : on  }",
        "line-length = 30\n[format]\nsequence-expand = 'auto'\nmapping-expand = 'auto'\n",
        "# a\na:\n  - &a0 ! {\n    a73: 'it''s \"x\": y',\n   }\n   # a\ncollection_0:\n  - &a1 ! {a73: on}\n",
    );
}

#[test]
fn empty_nested_collections_keep_their_tight_brackets() {
    formats(
        "a:\n    a: [  {  },  ]\n",
        "indent-width = 30\n[format]\nsequence-expand = 'always'\nbrace-spacing = true\n",
        "a:\n                              a:\n                                                            [\n                                                                                          {},\n                                                            ]\n",
    );
}

#[test]
fn flow_style_turns_an_overflowing_collection_into_block_level_by_level() {
    formats(
        "long: [alpha, beta, gamma, delta, epsilon]\nshort: [a, b]\n\
         map: {alpha: 1, beta: 2, gamma: 3, delta: 4}\n\
         nested: [[alpha, beta], [delta, epsilon, zeta, eta]]\n\
         deep: {a: {b: [alpha, beta, gamma, delta], c: 1}, d: [x]}\n\
         seq:\n  - [[alpha, beta, gamma, delta, epsilon], short]\n",
        "line-length = 30\n[format]\nsequence-style = 'flow'\nmapping-style = 'flow'\n",
        "long:\n  - alpha\n  - beta\n  - gamma\n  - delta\n  - epsilon\nshort: [a, b]\n\
         map:\n  alpha: 1\n  beta: 2\n  gamma: 3\n  delta: 4\n\
         nested:\n  - [alpha, beta]\n  - - delta\n    - epsilon\n    - zeta\n    - eta\n\
         deep:\n  a:\n    b:\n      - alpha\n      - beta\n      - gamma\n      - delta\n    c: 1\n  d: [x]\n\
         seq:\n  - - - alpha\n      - beta\n      - gamma\n      - delta\n      - epsilon\n    - short\n",
    );
}

// The nested mapping's line is 76 characters under its key, and would be 81 at the
// anchor's column.
#[test]
fn flow_style_measures_children_under_an_anchor_and_a_tag_from_their_key() {
    formats(
        "x: {o'41': &a3 !!str {\"9223372036854775808\": \"2002-12-14\", '0o+7': ! {':&\"{&🦀': \
         !!bool FALSE, \"--- x\": &a2 !!str '2002-12-14', z: '0o+7'}}}\n",
        "[format]\nsequence-style = 'block'\nmapping-style = 'flow'\n",
        "x:\n  o'41': &a3 !!str\n    '9223372036854775808': '2002-12-14'\n    0o+7: ! \
         {':&\"{&🦀': !!bool FALSE, --- x: &a2 !!str '2002-12-14', z: 0o+7}\n",
    );
}

// `"a}b"` needs quotes only in flow, so the child's line is 30 characters in block style
// and would be 32 with the flow spelling.
#[test]
fn flow_style_measures_a_converted_key_as_spelled_in_block() {
    formats(
        "q: {r: {\"a}b\": {c: xxxxxxxxxxxxxxxx}, s: yyyyyyyyyyyyyyyyyyyyyyyy}}\n",
        "line-length = 30\n[format]\nsequence-style = 'flow'\nmapping-style = 'flow'\n",
        "q:\n  r:\n    a}b: {c: xxxxxxxxxxxxxxxx}\n    s: yyyyyyyyyyyyyyyyyyyyyyyy\n",
    );
}

#[test]
fn flow_style_measures_a_requoted_entry_as_written_in_flow() {
    formats(
        "- key: \"世界 #x 'x' \t\\\" 'x'\"\n  other: x\n",
        "indent-width = 4\nline-length = 40\n[format]\nsequence-style = 'flow'\n\
         mapping-style = 'flow'\nsequence-expand = 'auto'\nmapping-expand = 'auto'\n",
        "- key: '世界 #x ''x'' \t\" ''x'''\n  other: x\n",
    );
}

#[test]
fn flow_style_under_always_leaves_block_collections_block() {
    formats(
        "a:\n  - x\n  - y\nb: [x, y]\n",
        "[format]\nsequence-style = 'flow'\nsequence-expand = 'always'\n",
        "a:\n  - x\n  - y\nb:\n  [\n    x,\n    y,\n  ]\n",
    );
}

#[test]
fn a_block_conversion_spreads_the_flow_collections_it_keeps() {
    formats(
        "a: {  a : &a0 [a]  }\r\ncollection_0: {  a : [\"café [value]\"]  }\r\nl: [[x], [y]]\r\n",
        "[format]\nmapping-style = 'block'\nsequence-expand = 'always'\n",
        "a:\n  a:\n    &a0 [\n      a,\n    ]\ncollection_0:\n  a:\n    [\n      'café [value]',\n    ]\n\
         l:\n  [\n    [\n      x,\n    ],\n    [\n      y,\n    ],\n  ]\n",
    );
    formats(
        "l: [[{a: 1}]]\n",
        "[format]\nsequence-style = 'block'\nmapping-expand = 'always'\n",
        "l: [[{a: 1}]]\n",
    );
}

#[test]
fn a_block_conversion_measures_nested_flow_with_final_spellings() {
    let input = "c: [ {  \"\\\"é,\":'it''s'  ,1:30: \"café \\\"quoted\\\": value\"  ,n'9_'_: !local 'first ---\u{feff}x'  } ]\n";
    let cfg = YamlLintConfig::from_toml_str(
        "[format]\nsequence-style = 'block'\nmapping-style = 'flow'\n",
    )
    .unwrap();
    let once = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(format_str(&once, &cfg, Path::new("case.yaml"), &[]), once);
    formats(
        "c:\n- [ true ,{\" é ]?>\"  :  \"[🦀&1'\"} ]\n",
        "indent-width = 37\nline-length = 50\n[format]\nsequence-style = 'block'\n\
         dash-on-own-line = true\nmapping-expand = 'auto'\n",
        &format!(
            "c:\n{0}- - true\n{0}  - {{\n{0}{1}' é ]?>': \"[🦀&1'\",\n{0}   }}\n",
            " ".repeat(37),
            " ".repeat(39),
        ),
    );
}

#[test]
fn flow_style_indents_a_converted_body_from_an_anchored_empty_key() {
    formats(
        "&a2 : [ '', ['it''s \"x\": y',&a3 'yes',\"--- x\",], ]\n",
        "indent-width = 4\nline-length = 40\n[format]\nsequence-style = 'flow'\n\
         mapping-style = 'flow'\nsequence-expand = 'auto'\nmapping-expand = 'auto'\n",
        "&a2 :\n    - ''\n    - ['it''s \"x\": y', &a3 'yes', --- x]\n",
    );
}

#[test]
fn an_explicit_keys_value_indents_from_the_mapping() {
    formats(
        "? &a2\n: [ -9223372036854775809, ['it''s \"x\": y',&a3 'yes',\"--- x\",], ]\n",
        "indent-width = 4\nline-length = 40\n[format]\nsequence-style = 'flow'\n\
         mapping-style = 'flow'\nsequence-expand = 'auto'\nmapping-expand = 'auto'\n",
        "? &a2\n:\n    - -9223372036854775809\n    - ['it''s \"x\": y', &a3 'yes', --- x]\n",
    );
}

#[test]
fn a_preserve_collection_spread_by_its_child_keeps_its_trailing_commas() {
    formats(
        "k:\n  - !!map\n    ? a\n    :\t{l: { é :  [ \"é\tvalue\" ,y ] }}\n",
        "[format]\nsequence-expand = 'always'\n",
        "k:\n  - !!map\n    ? a\n    :\n      {\n        l:\n          {\n            é:\n              [\n                é\tvalue,\n                y,\n              ],\n          },\n      }\n",
    );
}

#[test]
fn always_keeps_a_converted_implicit_pair_block() {
    formats(
        "key: [a: '>\t1\"']\n",
        "[format]\nsequence-style = 'block'\nmapping-style = 'flow'\nmapping-expand = 'always'\n",
        "key:\n  - a: '>\t1\"'\n",
    );
}

#[test]
fn a_collection_with_an_entry_that_cannot_be_respelled_stays_as_written() {
    let format = |input: &str, toml: &str| {
        let cfg = YamlLintConfig::from_toml_str(toml).expect("expand config parses");
        format_str(input, &cfg, Path::new("case.yaml"), &[])
    };
    // Preserved quoting keeps a multi-line quoted scalar on its lines.
    let preserve = "line-length = 20\n[format]\nquote-style = 'preserve'\n";
    let always = "line-length = 20\n[format]\nquote-style = 'preserve'\n\
                  sequence-expand = 'always'\nmapping-expand = 'always'\n";
    for (input, base, toml) in [
        ("k: {[a]: b, c: d}\n", preserve, always),
        ("k: {\"a\n  b\": c}\n", preserve, always),
        ("k: [&a\n  !t [x]]\n", preserve, always),
        (
            "k: [&a\n  !t [x]]\n",
            preserve,
            "line-length = 20\n[format]\nquote-style = 'preserve'\nsequence-expand = 'never'\n",
        ),
        ("k: &a\n  !t [x]\n", preserve, always),
        ("k: [\"a\n  b\", c]\n", preserve, always),
        ("k: {a: \"x\n  y\"}\n", preserve, always),
        ("k: [[\"a\n  b\"]]\n", preserve, always),
        (
            "a: {b: [\"x\n  y\"]}\n",
            "[format]\nquote-style = 'preserve'\nmapping-style = 'block'\n",
            "[format]\nquote-style = 'preserve'\nmapping-style = 'block'\n\
             sequence-expand = 'always'\n",
        ),
        (
            "a: [{b: \"x\n  y\"}]\n",
            "[format]\nquote-style = 'preserve'\nsequence-style = 'block'\n",
            "[format]\nquote-style = 'preserve'\nsequence-style = 'block'\n\
             mapping-expand = 'always'\n",
        ),
    ] {
        let output = format(input, toml);
        assert_eq!(
            output,
            format(input, base),
            "input {input:?} under {toml:?}"
        );
        assert_eq!(format(&output, toml), output, "second run of {output:?}");
    }
}

#[test]
fn a_refused_conversion_is_placed_after_a_reshaped_one() {
    let input = "- k: [x, y]\n- [p, q]  # c\n";
    let cfg = YamlLintConfig::from_toml_str(
        "[format]\nsequence-style = 'block'\ndash-on-own-line = true\n",
    )
    .unwrap();
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "-\n  k:\n    - x\n    - y\n- [p, q]  # c\n");
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn reshaped_dash_positions_do_not_block_another_scalars_folding() {
    formats(
        "- k: v\n- long value\n- k: v # c\n",
        "line-length = 10\n[format]\nfold-long-lines = true\ndash-on-own-line = true\n",
        "-\n  k: v\n- long\n  value\n- k: v  # c\n",
    );
}

#[test]
fn reshaped_dash_positions_keep_their_own_scalar_comments_in_place() {
    for input in [
        "- k: v\n- k: long value # c\n",
        "- {k: v}\n- k: long value # c\n",
    ] {
        formats(
            input,
            "line-length = 10\n[format]\nfold-long-lines = true\ndash-on-own-line = true\nmapping-style = 'block'\n",
            "-\n  k: v\n- k: long value  # c\n",
        );
    }
}

#[test]
fn folding_leaves_a_scalar_whose_comment_keeps_its_dash() {
    let input = "? k\n: - double: \"it's a long value: y\"  # w\n    b: 1\n";
    for style in ["", "sequence-style = 'block'\n"] {
        formats(
            input,
            &format!(
                "line-length = 30\n[format]\nfold-long-lines = true\n{style}\
                 indent-sequences = false\ndash-on-own-line = true\n"
            ),
            input,
        );
    }
}

#[test]
fn an_empty_value_keeps_its_colon_with_its_own_key() {
    formats(
        "{a: , b: c}\n",
        "[format]\nmapping-expand = 'always'\n",
        "{\n  a:,\n  b: c,\n}\n",
    );
}

#[test]
fn a_converted_implicit_pair_is_measured_where_flowed_measures_it() {
    formats(
        "k: [a: b, xxxxx]\n",
        "indent-width = 3\nline-length = 11\n[format]\nsequence-style = 'flow'\n\
         mapping-style = 'flow'\ndash-on-own-line = true\n",
        "k:\n   - {a: b}\n   - xxxxx\n",
    );
}

#[test]
fn an_alias_key_keeps_its_space_before_the_colon() {
    formats(
        "a: &a x\nk: {*a : b}\n",
        "[format]\nmapping-expand = 'always'\n",
        "a: &a x\nk:\n  {\n    *a : b,\n  }\n",
    );
}

#[test]
fn a_converted_implicit_pair_with_a_collection_value_stays_block() {
    formats(
        "key: [a: {b: c}]\n",
        "[format]\nsequence-style = 'block'\nmapping-style = 'flow'\n",
        "key:\n  - a: {b: c}\n",
    );
}

const PROPERTIES: &str = "f: &a !!seq [x, y]\ng: !!map &b {k: v}\ns: &c !!str |\n  text\nm: !!map &d\n  k: v\n";

#[test]
fn property_order_anchor_first_puts_each_anchor_before_its_tag() {
    formats(
        PROPERTIES,
        "[format]\nproperty-order = 'anchor-first'\n",
        "f: &a !!seq [x, y]\ng: &b !!map {k: v}\ns: &c !!str |\n  text\nm: &d !!map\n  k: v\n",
    );
}

#[test]
fn property_order_tag_first_puts_each_tag_before_its_anchor() {
    formats(
        PROPERTIES,
        "[format]\nproperty-order = 'tag-first'\n",
        "f: !!seq &a [x, y]\ng: !!map &b {k: v}\ns: !!str &c |\n  text\nm: !!map &d\n  k: v\n",
    );
}

#[test]
fn property_order_preserve_keeps_each_order() {
    formats(
        PROPERTIES,
        "[format]\nproperty-order = 'preserve'\n",
        PROPERTIES,
    );
}

#[test]
fn never_collapses_and_preserve_keeps_the_other_kind() {
    formats(
        "a: [x,\n  y]\nb: {k: [p,\n  q]}\nc: {k: 1,\n  j: 2}\n",
        "[format]\nsequence-expand = 'never'\n",
        "a: [x, y]\nb: {k: [p, q]}\nc: {k: 1,\n    j: 2}\n",
    );
}

#[test]
fn an_anchored_item_under_dash_on_own_line_is_measured_beside_its_dash() {
    formats(
        "k:\n  - &a {b: {c: [xx, yy]}}\n",
        "indent-width = 4\nline-length = 22\n[format]\nmapping-style = 'block'\n\
         dash-on-own-line = true\nsequence-expand = 'auto'\nmapping-expand = 'auto'\n",
        "k:\n    - &a\n      b:\n          c: [xx, yy]\n",
    );
}

#[test]
fn a_quoted_key_ending_in_a_tag_like_word_is_not_split_from_its_value() {
    formats(
        "k:\n  - a:  # c\n      \"\u{feff}e !-\":\n        [\n          x,\n        ]\n",
        "line-length = 16\n[format]\nquote-style = 'double'\nsequence-expand = 'auto'\n",
        "k:\n  - a:  # c\n      \"\u{feff}e !-\":\n        [x]\n",
    );
}
