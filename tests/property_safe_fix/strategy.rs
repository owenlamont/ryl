//! Proptest strategies that build random `Document` values from the AST.

use proptest::prelude::*;

use super::ast::{
    BlockBodyLine, BlockEntry, BlockScalarSpec, ColonGap, Document, FlowStyle,
    InlineComment, Layout, MultilineFlowSpec, MultilineLine, MultilinePlainSpec,
    MultilineQuoteStyle, MultilineQuotedSpec, NewlineStyle, Node, Scalar, SeqBody,
    SeqItem,
};

fn arb_plain_identifier() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,6}".prop_map(|value| value)
}

fn arb_multibyte_char() -> impl Strategy<Value = char> {
    prop_oneof![Just('é'), Just('—'), Just('世'), Just('🦀')]
}

fn arb_plain_value() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_']{0,6}".prop_map(|value| value)
}

fn arb_quoted_payload() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just('a'),
            Just('b'),
            Just('1'),
            Just(' '),
            Just('#'),
            Just(','),
            Just('{'),
            Just('}'),
            Just('['),
            Just(']'),
            Just('*'),
            Just('?'),
            Just('&'),
            Just('!'),
            Just(':'),
            arb_multibyte_char(),
        ],
        0usize..=6,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

// Scalars YAML 1.1 resolves to a non-string but YAML 1.2 reads as a string, generated
// quoted too so the keep-quotes path runs.
fn arb_yaml_1_1_ambiguous() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("no".to_string()),
        Just("yes".to_string()),
        Just("on".to_string()),
        Just("off".to_string()),
        Just("0b101".to_string()),
        Just("1:30".to_string()),
        Just("0o17".to_string()),
        Just("2002-12-14".to_string()),
        Just("1_000".to_string()),
        Just("<<".to_string()),
        Just("=".to_string()),
    ]
}

// Integers past `i64` and a sign after a radix prefix, where the loader's int spelling
// check and `quoted-strings` must agree on whether quotes are load-bearing.
fn arb_core_int_edge() -> impl Strategy<Value = String> {
    prop::sample::select(
        &[
            "9223372036854775808",
            "-9223372036854775809",
            "0x8000000000000000",
            "0o1000000000000000000000",
            "0x-1",
            "0o+7",
        ][..],
    )
    .prop_map(str::to_owned)
}

fn arb_bool_spelling() -> impl Strategy<Value = String> {
    prop::sample::select(
        &["true", "True", "TRUE", "false", "False", "FALSE", "tRUE"][..],
    )
    .prop_map(str::to_owned)
}

fn arb_tagged_bool_spelling() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("!!str"), Just("!!bool")],
        arb_bool_spelling(),
    )
        .prop_map(|(tag, spelling)| format!("{tag} {spelling}"))
}

fn arb_scalar() -> impl Strategy<Value = Scalar> {
    prop_oneof![
        4 => arb_plain_value().prop_map(Scalar::Plain),
        4 => arb_quoted_payload().prop_map(Scalar::SingleQuoted),
        4 => arb_quoted_payload().prop_map(Scalar::DoubleQuoted),
        1 => arb_yaml_1_1_ambiguous().prop_map(Scalar::Plain),
        1 => arb_yaml_1_1_ambiguous().prop_map(Scalar::SingleQuoted),
        1 => arb_yaml_1_1_ambiguous().prop_map(Scalar::DoubleQuoted),
        1 => arb_bool_spelling().prop_map(Scalar::Plain),
        1 => arb_bool_spelling().prop_map(Scalar::SingleQuoted),
        1 => arb_bool_spelling().prop_map(Scalar::DoubleQuoted),
        1 => arb_tagged_bool_spelling().prop_map(Scalar::Plain),
        1 => arb_core_int_edge().prop_map(Scalar::Plain),
        1 => arb_core_int_edge().prop_map(Scalar::SingleQuoted),
        1 => arb_core_int_edge().prop_map(Scalar::DoubleQuoted),
    ]
}

fn arb_flow_style() -> impl Strategy<Value = FlowStyle> {
    (0u8..=2, 0u8..=2, 0u8..=2, 0u8..=2, 0u8..=2).prop_map(
        |(
            inner_padding,
            spaces_before_comma,
            spaces_after_comma,
            spaces_before_colon,
            spaces_after_colon,
        )| {
            FlowStyle {
                inner_padding,
                spaces_before_comma,
                spaces_after_comma,
                spaces_before_colon,
                spaces_after_colon,
            }
        },
    )
}

fn arb_colon_gap() -> impl Strategy<Value = ColonGap> {
    prop_oneof![
        3 => Just(ColonGap::default()),
        2 => (
            0u8..=2,
            0u8..=2,
            prop::bool::weighted(0.2),
            prop::option::weighted(0.3, 1u8..=3),
        )
            .prop_map(|(extra_before, extra_after, tab, explicit)| ColonGap {
                extra_before,
                extra_after,
                tab,
                explicit,
            }),
    ]
}

fn arb_layout() -> impl Strategy<Value = Layout> {
    (
        prop_oneof![3 => Just(2u8), 2 => 1u8..=5],
        prop::bool::weighted(0.3),
        prop_oneof![3 => Just(0i8), 1 => -2i8..=3],
    )
        .prop_map(|(width, flush, comment_shift)| Layout {
            width,
            flush,
            comment_shift,
        })
}

fn arb_seq_item(depth: u32) -> impl Strategy<Value = SeqItem> {
    let body = prop_oneof![
        4 => arb_node().prop_map(SeqBody::Inline),
        1 => arb_block_scalar_spec().prop_map(SeqBody::BlockScalar),
        1 => arb_multiline_quoted_spec().prop_map(SeqBody::MultilineQuoted),
        1 => arb_multiline_plain_spec().prop_map(SeqBody::MultilinePlain),
        1 => (
            prop::sample::select(&["!!map", "!local", "&m", ""][..]),
            prop::collection::vec(arb_nested_entry(depth), 1..=2),
        )
            .prop_map(|(property, entries)| SeqBody::TaggedMap(property, entries)),
        2 => prop::collection::vec(arb_nested_entry(depth), 1..=3)
            .prop_map(SeqBody::CompactMap),
        2 => prop::collection::vec((1u8..=3, arb_node()), 1..=3)
            .prop_map(SeqBody::CompactSeq),
    ];
    (1u8..=3, prop_oneof![3 => Just(2u8), 1 => 1u8..=4], body).prop_map(
        |(dash_spaces, width, body)| SeqItem {
            dash_spaces,
            width,
            body,
        },
    )
}

fn arb_node() -> impl Strategy<Value = Node> {
    let leaf = arb_scalar().prop_map(Node::Scalar);
    leaf.prop_recursive(2, 16, 4, |inner| {
        prop_oneof![
            (
                prop::collection::vec(inner.clone(), 0..=4),
                arb_flow_style()
            )
                .prop_map(|(items, style)| Node::FlowSeq(items, style)),
            (
                prop::collection::vec((arb_scalar(), inner), 0..=4),
                arb_flow_style(),
            )
                .prop_map(|(pairs, style)| Node::FlowMap(pairs, style)),
        ]
    })
}

/// A block entry's value: scalars and multi-line forms, plus nested block collections
/// while `depth` lasts.
fn arb_block_value(depth: u32) -> BoxedStrategy<Node> {
    let leaves = prop_oneof![
        10 => arb_node(),
        3 => arb_block_scalar_spec().prop_map(Node::BlockScalar),
        3 => arb_multiline_quoted_spec().prop_map(Node::MultilineQuoted),
        3 => arb_multiline_plain_spec().prop_map(Node::MultilinePlain),
        2 => arb_multiline_flow_spec().prop_map(Node::MultilineFlowSeq),
    ];
    let Some(below) = depth.checked_sub(1) else {
        return leaves.boxed();
    };
    prop_oneof![
        21 => leaves,
        3 => prop::collection::vec(arb_nested_entry(below), 1..=3).prop_map(Node::BlockMap),
        4 => prop::collection::vec(arb_seq_item(below), 1..=3).prop_map(Node::BlockSeq),
    ]
    .boxed()
}

fn arb_multiline_flow_spec() -> impl Strategy<Value = MultilineFlowSpec> {
    (
        prop::collection::vec((0u8..=3, arb_scalar()), 1..=3),
        0u8..=2,
    )
        .prop_map(|(items, closer)| MultilineFlowSpec { items, closer })
}

fn arb_multiline_plain_spec() -> impl Strategy<Value = MultilinePlainSpec> {
    (
        "[a-z][a-z0-9]{0,5}",
        prop::collection::vec(arb_multiline_line("[a-z][a-z0-9]{0,5}"), 1..=3),
        0u8..=3,
    )
        .prop_map(|(first, continuations, extra)| MultilinePlainSpec {
            first,
            continuations,
            extra,
        })
}

fn arb_block_scalar_spec() -> impl Strategy<Value = BlockScalarSpec> {
    (
        prop_oneof![Just(""), Just("!!str "), Just("&blk ")],
        prop_oneof![Just('|'), Just('>')],
        prop::option::of(prop_oneof![Just('-'), Just('+')]),
        prop::option::of(1u8..=4u8),
        prop_oneof![3 => Just(2u8), 1 => 1u8..=4],
        prop::option::weighted(0.5, 0u8..=2),
        prop::option::of(arb_block_body_content()),
        prop::collection::vec(arb_block_body_line(), 0..=3),
        prop::option::weighted(0.2, 0u8..=1),
        prop::bool::weighted(0.1),
        0usize..=3,
        0u8..=3,
    )
        .prop_map(
            |(
                properties,
                style,
                chomp,
                explicit_indent,
                offset,
                leading_short,
                first,
                rest,
                trailing_comment,
                blank_only,
                trailing_blanks,
                header_spaces,
            )| {
                let mut body: Vec<_> = first.into_iter().collect();
                body.extend(rest);
                if blank_only {
                    body.retain(|line| !matches!(line, BlockBodyLine::Content { .. }));
                    body.push(BlockBodyLine::Spaces(2));
                }
                // granit keeps a last whitespace-only line under clip as a line break,
                // which yaml, ruamel and PyYAML (and the spec) chomp.
                while chomp != Some('+')
                    && matches!(body.last(), Some(BlockBodyLine::Spaces(_)))
                {
                    body.pop();
                }
                body.extend(std::iter::repeat_n(BlockBodyLine::Blank, trailing_blanks));
                BlockScalarSpec {
                    properties,
                    style,
                    chomp,
                    explicit_indent,
                    header_spaces,
                    offset,
                    leading_short,
                    body,
                    trailing_comment,
                }
            },
        )
}

fn arb_block_body_content() -> impl Strategy<Value = BlockBodyLine> {
    ("#?[a-z][a-z0-9]{0,6}", 0u8..=3)
        .prop_map(|(text, trailing_ws)| BlockBodyLine::Content { text, trailing_ws })
}

fn arb_block_body_line() -> impl Strategy<Value = BlockBodyLine> {
    prop_oneof![
        6 => arb_block_body_content(),
        2 => Just(BlockBodyLine::Blank),
        1 => (-2i8..=2).prop_map(BlockBodyLine::Spaces),
    ]
}

fn arb_multiline_quoted_spec() -> impl Strategy<Value = MultilineQuotedSpec> {
    (
        prop_oneof![
            Just(MultilineQuoteStyle::Single),
            Just(MultilineQuoteStyle::Double),
        ],
        prop::collection::vec(arb_multiline_line("#?[a-z][a-z0-9]{0,5}"), 1..=4),
        0u8..=3,
    )
        .prop_map(|(style, lines, extra)| MultilineQuotedSpec {
            style,
            lines,
            extra,
        })
}

fn arb_multiline_line(content: &'static str) -> impl Strategy<Value = MultilineLine> {
    prop_oneof![
        3 => content.prop_map(MultilineLine::Content),
        1 => Just(MultilineLine::Blank),
    ]
}

fn arb_inline_comment() -> impl Strategy<Value = InlineComment> {
    (
        "[ \t]{1,6}",
        0u8..=2,
        "[a-z][a-z0-9 ]{0,8}",
        prop::option::of(arb_multibyte_char()),
    )
        .prop_map(
            |(whitespace_before_hash, spaces_after_hash, mut text, multibyte)| {
                if let Some(ch) = multibyte {
                    text.push(ch);
                }
                InlineComment {
                    whitespace_before_hash,
                    spaces_after_hash,
                    text,
                }
            },
        )
}

fn arb_leading_comment() -> impl Strategy<Value = Option<String>> {
    prop::option::weighted(0.3, "[a-z][a-z0-9]{0,6}")
}

/// Keys from a two-letter alphabet half the time, so nested mappings are often
/// out of order or hold duplicates.
fn arb_nested_entry(depth: u32) -> impl Strategy<Value = BlockEntry> {
    (
        arb_leading_comment(),
        prop_oneof![arb_plain_identifier(), "[ab]"],
        arb_colon_gap(),
        prop_oneof![3 => arb_node().boxed(), 1 => arb_block_value(depth)],
        prop::option::of(arb_inline_comment()),
        arb_layout(),
    )
        .prop_map(
            |(leading_comment, key, colon, value, trailing_inline_comment, layout)| {
                BlockEntry {
                    leading_comment,
                    key,
                    colon,
                    value,
                    trailing_inline_comment,
                    layout,
                }
            },
        )
}

fn arb_block_entry() -> impl Strategy<Value = BlockEntry> {
    (
        arb_leading_comment(),
        prop_oneof![
            8 => arb_plain_identifier(),
            1 => arb_bool_spelling(),
            1 => arb_core_int_edge(),
        ],
        arb_colon_gap(),
        arb_block_value(2),
        prop::option::of(arb_inline_comment()),
        arb_layout(),
    )
        .prop_map(
            |(leading_comment, key, colon, value, trailing_inline_comment, layout)| {
                BlockEntry {
                    leading_comment,
                    key,
                    colon,
                    value,
                    trailing_inline_comment,
                    layout,
                }
            },
        )
}

pub fn arb_document() -> impl Strategy<Value = Document> {
    (
        prop_oneof![
            3 => Just(None),
            1 => Just(Some((1, 1))),
            1 => Just(Some((1, 2))),
        ],
        prop::collection::vec(arb_block_entry(), 1..=4),
        prop_oneof![
            Just(NewlineStyle::Lf),
            Just(NewlineStyle::Crlf),
            Just(NewlineStyle::Cr)
        ],
        any::<bool>(),
    )
        .prop_map(|(version_directive, entries, newline, has_final_newline)| {
            Document {
                version_directive,
                entries,
                newline,
                has_final_newline,
            }
        })
}
