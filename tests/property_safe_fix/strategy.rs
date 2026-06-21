//! Proptest strategies that build random `Document` values from the AST.

use proptest::prelude::*;

use super::ast::{
    BlockBodyLine, BlockEntry, BlockScalarSpec, Document, FlowStyle, InlineComment,
    MultilineLine, MultilinePlainSpec, MultilineQuoteStyle, MultilineQuotedSpec,
    NewlineStyle, Node, Scalar,
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

// Scalars YAML 1.1 resolves to a non-string but YAML 1.2 reads as a string, so quoting
// them is redundant under 1.2 but load-bearing under an explicit `%YAML 1.1`. Generating
// them in quoted form lets the directive prelude exercise the keep-quotes-under-1.1 path.
fn arb_yaml_1_1_ambiguous() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("no".to_string()),
        Just("yes".to_string()),
        Just("on".to_string()),
        Just("off".to_string()),
        Just("y".to_string()),
        Just("0b101".to_string()),
        Just("1:30".to_string()),
        Just("0o17".to_string()),
        Just("2002-12-14".to_string()),
    ]
}

fn arb_scalar() -> impl Strategy<Value = Scalar> {
    prop_oneof![
        4 => arb_plain_value().prop_map(Scalar::Plain),
        4 => arb_quoted_payload().prop_map(Scalar::SingleQuoted),
        4 => arb_quoted_payload().prop_map(Scalar::DoubleQuoted),
        1 => arb_yaml_1_1_ambiguous().prop_map(Scalar::Plain),
        1 => arb_yaml_1_1_ambiguous().prop_map(Scalar::SingleQuoted),
        1 => arb_yaml_1_1_ambiguous().prop_map(Scalar::DoubleQuoted),
    ]
}

fn arb_flow_style() -> impl Strategy<Value = FlowStyle> {
    (0u8..=2, 0u8..=2, 0u8..=2, any::<bool>()).prop_map(
        |(
            inner_padding,
            spaces_before_comma,
            spaces_after_comma,
            space_after_colon,
        )| {
            FlowStyle {
                inner_padding,
                spaces_before_comma,
                spaces_after_comma,
                space_after_colon,
            }
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

fn arb_top_level_node() -> impl Strategy<Value = Node> {
    prop_oneof![
        10 => arb_node(),
        3 => arb_block_scalar_spec().prop_map(Node::BlockScalar),
        3 => arb_multiline_quoted_spec().prop_map(Node::MultilineQuoted),
        3 => arb_multiline_plain_spec().prop_map(Node::MultilinePlain),
    ]
}

fn arb_multiline_plain_spec() -> impl Strategy<Value = MultilinePlainSpec> {
    (
        "[a-z][a-z0-9]{0,5}",
        prop::collection::vec(arb_multiline_line(), 1..=3),
    )
        .prop_map(|(first, continuations)| MultilinePlainSpec {
            first,
            continuations,
        })
}

fn arb_block_scalar_spec() -> impl Strategy<Value = BlockScalarSpec> {
    (
        prop_oneof![Just('|'), Just('>')],
        prop::option::of(prop_oneof![Just('-'), Just('+')]),
        prop::option::of(2u8..=4u8),
        arb_block_body_content(),
        prop::collection::vec(arb_block_body_line(), 0..=3),
    )
        .prop_map(|(style, chomp, explicit_indent, first, rest)| {
            let mut body = vec![first];
            body.extend(rest);
            BlockScalarSpec {
                style,
                chomp,
                explicit_indent,
                body,
            }
        })
}

fn arb_block_body_content() -> impl Strategy<Value = BlockBodyLine> {
    ("[a-z][a-z0-9]{0,6}", 0u8..=3)
        .prop_map(|(text, trailing_ws)| BlockBodyLine::Content { text, trailing_ws })
}

fn arb_block_body_line() -> impl Strategy<Value = BlockBodyLine> {
    prop_oneof![
        3 => arb_block_body_content(),
        1 => Just(BlockBodyLine::Blank),
    ]
}

fn arb_multiline_quoted_spec() -> impl Strategy<Value = MultilineQuotedSpec> {
    (
        prop_oneof![
            Just(MultilineQuoteStyle::Single),
            Just(MultilineQuoteStyle::Double),
        ],
        prop::collection::vec(arb_multiline_line(), 1..=4),
    )
        .prop_map(|(style, lines)| MultilineQuotedSpec { style, lines })
}

fn arb_multiline_line() -> impl Strategy<Value = MultilineLine> {
    prop_oneof![
        3 => "[a-z][a-z0-9]{0,5}".prop_map(MultilineLine::Content),
        1 => Just(MultilineLine::Blank),
    ]
}

fn arb_inline_comment() -> impl Strategy<Value = InlineComment> {
    (
        0u8..=2,
        "[a-z][a-z0-9 ]{0,8}",
        prop::option::of(arb_multibyte_char()),
    )
        .prop_map(|(spaces_after_hash, mut text, multibyte)| {
            if let Some(ch) = multibyte {
                text.push(ch);
            }
            InlineComment {
                spaces_after_hash,
                text,
            }
        })
}

fn arb_block_entry() -> impl Strategy<Value = BlockEntry> {
    (
        arb_plain_identifier(),
        arb_top_level_node(),
        prop::option::of(arb_inline_comment()),
    )
        .prop_map(|(key, value, trailing_inline_comment)| BlockEntry {
            key,
            value,
            trailing_inline_comment,
        })
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
