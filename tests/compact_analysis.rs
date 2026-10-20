// Canonical paths merge reconstruction coverage with the production source files.
mod events {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/text_version/events.rs"
    ));
}
mod lexical {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/text_version/lexical.rs"
    ));
}
#[expect(
    clippy::excessive_nesting,
    reason = "include! adds artificial nesting to the production module"
)]
mod spans {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/text_version/spans.rs"
    ));
}

use granit_parser::{
    Event, Marker, Parser, ScalarStyle, Scanner, Span, StrInput, TokenType,
};
use std::borrow::Cow;

const INPUTS: &[&str] = &[
    "",
    "\u{feff}café: [世, 🦀, 'é'] # fin\r\nnext:\r  -\r  - \"a\\nb\"\r",
    "%YAML 1.1\n%TAG !e! tag:example.org,2026:\n---\n!e!map &m {a: !e!scalar x, b: !e!seq [1]}\n...\n---\na: &a yes\nb: *a\n",
    "? [a, b]\n: {x: 1}\nblock: |+\n  café\n\nfold: >-\n  hello\n  world\nempty: |\nnext: # own\n  # above\n  null\n",
];

#[test]
fn streaming_verification_keeps_exact_events_and_ignores_layout() {
    for &source in INPUTS {
        let expected = Parser::new_from_str(source).map(|item| item.unwrap().0);
        let mut observed = Vec::new();
        assert!(
            events::verify_strict(expected, source, |event, span| {
                observed.push((event, span));
            })
            .unwrap()
        );
        assert_eq!(
            observed,
            Parser::new_from_str(source)
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        );
    }
    let expected = Parser::new_from_str("é:\n    - &a café\n    - *a\n")
        .map(|item| item.unwrap().0);
    assert!(
        events::verify_strict(
            expected,
            "# heading\né:\n  - &a café # inline\n  - *a\n",
            |_, _| {}
        )
        .unwrap()
    );
}

#[test]
fn streaming_verification_rejects_each_significant_event_difference() {
    for (source, output) in [
        ("a: café\n", "a: cafe\n"),
        ("a: café\n", "a: 'café'\n"),
        ("a: !!str café\n", "a: café\n"),
        ("a: [b]\n", "a:\n  - b\n"),
        ("a: {b: c}\n", "a:\n  b: c\n"),
        ("a: &a café\nb: *a\n", "a: café\nb: café\n"),
        (
            "a: &a café\nb: &b café\nc: *a\n",
            "a: &a café\nb: &b café\nc: *b\n",
        ),
        ("%YAML 1.1\n---\na: yes\n", "%YAML 1.2\n---\na: yes\n"),
        ("a: b\n", "---\na: b\n"),
        ("a: b\n", "a: b\n---\nc: d\n"),
        ("a: b\n---\nc: d\n", "a: b\n"),
        ("a: b\n", ""),
        ("", "a: b\n"),
    ] {
        let expected = Parser::new_from_str(source).map(|item| item.unwrap().0);
        assert!(
            !events::verify_strict(expected, output, |_, _| {}).unwrap(),
            "{source:?}: {output:?}"
        );
    }
}

#[test]
fn streaming_verification_parses_strictly_through_mismatches_and_suffixes() {
    for output in [
        "a: [first,\nsecond]\n",
        "a: b\n---\nbad: [\n",
        "x: y\n---\nbad: [\n",
    ] {
        let expected = Parser::new_from_str("a: b\n").map(|item| item.unwrap().0);
        assert!(
            events::verify_strict(expected, output, |_, _| {}).is_err(),
            "{output:?}"
        );
    }
}

fn assert_span_eq(actual: Span, expected: Span) {
    assert_eq!(actual, expected);
    assert_eq!(actual.start.byte_offset(), expected.start.byte_offset());
    assert_eq!(actual.end.byte_offset(), expected.end.byte_offset());
    assert_eq!(
        actual.tag_start().map(|marker| marker.byte_offset()),
        expected.tag_start().map(|marker| marker.byte_offset())
    );
}

#[test]
fn every_event_and_span_reconstructs_exactly() {
    for &source in INPUTS {
        for pair in Parser::new_from_str(source) {
            let (event, span) = pair.unwrap();
            let significant = !matches!(
                event,
                Event::Comment(..) | Event::StreamStart | Event::StreamEnd
            );
            let stored = events::StoredEvent::new(source, event.clone());
            assert_eq!(stored.borrow(source), event, "{source:?}");
            assert_eq!(stored.significant(), significant);
            assert!(stored.equivalent(
                &events::StoredEvent::new(source, event),
                source,
                source
            ));
            assert_span_eq(spans::StoredSpan::new(span).get(), span);
        }
    }
}

#[test]
fn scanner_spans_preserve_unicode_and_zero_width_tokens() {
    let mut zero_width = 0;
    for &source in INPUTS {
        let tokens = lexical::scan(source);
        let mut reconstructed = tokens.iter();
        for pair in Scanner::new(StrInput::new(source)) {
            let (original, token) = pair.unwrap().into_parts();
            let (span, kind) = reconstructed.next().unwrap();
            match (kind, token) {
                (lexical::Kind::StreamStart, TokenType::StreamStart)
                | (lexical::Kind::StreamEnd, TokenType::StreamEnd)
                | (lexical::Kind::DocumentStart, TokenType::DocumentStart)
                | (lexical::Kind::DocumentEnd, TokenType::DocumentEnd)
                | (lexical::Kind::BlockSequenceStart, TokenType::BlockSequenceStart)
                | (lexical::Kind::BlockMappingStart, TokenType::BlockMappingStart)
                | (lexical::Kind::BlockEnd, TokenType::BlockEnd)
                | (lexical::Kind::FlowSequenceStart, TokenType::FlowSequenceStart)
                | (lexical::Kind::FlowSequenceEnd, TokenType::FlowSequenceEnd)
                | (lexical::Kind::FlowMappingStart, TokenType::FlowMappingStart)
                | (lexical::Kind::FlowMappingEnd, TokenType::FlowMappingEnd)
                | (lexical::Kind::BlockEntry, TokenType::BlockEntry)
                | (lexical::Kind::Key, TokenType::Key)
                | (lexical::Kind::Value, TokenType::Value)
                | (lexical::Kind::Alias, TokenType::Alias(_))
                | (lexical::Kind::Anchor, TokenType::Anchor(_))
                | (lexical::Kind::Tag, TokenType::Tag(..))
                | (
                    lexical::Kind::Other,
                    TokenType::VersionDirective(..)
                    | TokenType::TagDirective(..)
                    | TokenType::FlowEntry,
                ) => {}
                (
                    lexical::Kind::Scalar { style, empty },
                    TokenType::Scalar(original_style, value),
                ) => {
                    assert_eq!(*style, original_style);
                    assert_eq!(*empty, value.is_empty());
                }
                (
                    lexical::Kind::Comment { text, placement },
                    TokenType::Comment(comment),
                ) => {
                    assert_eq!(*placement, comment.placement());
                    assert_eq!(text.as_ref(), comment.text());
                }
                (_, token) => panic!("incorrect stored kind for {token:?}"),
            }
            assert_span_eq(span, original);
            zero_width += usize::from(span.start == span.end);
        }
        assert!(reconstructed.next().is_none());
        assert!(matches!(
            tokens.iter().next().unwrap().1,
            lexical::Kind::StreamStart
        ));
        assert_eq!(tokens.iter_from(1).count() + 1, tokens.iter().count());
    }
    assert!(zero_width > 10);
}

#[test]
fn compact_spans_preserve_absent_byte_offsets_and_indent() {
    for indent in [None, Some(0), Some(17)] {
        let span =
            Span::new(Marker::new(0, 1, 0), Marker::new(3, 2, 1)).with_indent(indent);
        assert_span_eq(spans::StoredSpan::new(span).get(), span);
    }
}

#[test]
fn wide_spans_preserve_sentinel_and_large_offsets() {
    for value in [u32::MAX as usize, usize::MAX] {
        for span in [
            Span::empty(Marker::new(value, 1, 0)),
            Span::empty(Marker::new(0, value, 0)),
            Span::empty(Marker::new(0, 1, value)),
            Span::empty(Marker::new(0, 1, 0).with_byte_offset(Some(value))),
            Span::empty(Marker::new(0, 1, 0)).with_indent(Some(value)),
        ] {
            let stored = spans::StoredSpan::new(span);
            assert!(matches!(stored, spans::StoredSpan::Wide(_)));
            assert_span_eq(stored.get(), span);
        }
    }
}

#[test]
fn borrowed_scalar_outside_source_survives_reconstruction() {
    let event = Event::Scalar(Cow::Borrowed("outside"), ScalarStyle::Plain, 0, None);
    let stored = events::StoredEvent::new("input", event.clone());
    assert_eq!(stored.borrow("input"), event);
}

#[test]
fn compact_records_are_smaller_than_parser_records() {
    assert!(size_of::<spans::StoredSpan>() < size_of::<Span>());
    assert!(size_of::<events::StoredEvent>() < size_of::<Event<'_>>());
}
