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
mod spans {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/text_version/spans.rs"
    ));
}

use ryl_yaml_parser::{
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
fn scanner_failure_has_no_stream_end_certificate() {
    let complete = "key: value\n";
    let incomplete = "key: ['unterminated";
    assert!(
        lexical::scan(complete)
            .iter(complete)
            .any(|(_, kind)| matches!(kind, lexical::Kind::StreamEnd))
    );
    assert!(
        !lexical::scan(incomplete)
            .iter(incomplete)
            .any(|(_, kind)| matches!(kind, lexical::Kind::StreamEnd))
    );
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
            let mut tables = events::Tables::default();
            let stored = events::StoredEvent::new(source, event.clone(), &mut tables);
            assert_eq!(stored.borrow(source, &tables), event, "{source:?}");
            let mut spans = spans::Spans::default();
            let stored = spans.store(&span);
            assert_span_eq(spans.get(stored), span);
        }
    }
}

#[test]
fn scanner_spans_preserve_unicode_and_zero_width_tokens() {
    let mut zero_width = 0;
    for &source in INPUTS {
        let tokens = lexical::scan(source);
        let mut reconstructed = tokens.iter(source);
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
                | (lexical::Kind::FlowEntry, TokenType::FlowEntry)
                | (lexical::Kind::BlockEntry, TokenType::BlockEntry)
                | (lexical::Kind::Key, TokenType::Key)
                | (lexical::Kind::Value, TokenType::Value)
                | (lexical::Kind::Alias, TokenType::Alias(_))
                | (lexical::Kind::Anchor, TokenType::Anchor(_))
                | (lexical::Kind::Tag, TokenType::Tag(..))
                | (
                    lexical::Kind::Other,
                    TokenType::VersionDirective(..) | TokenType::TagDirective(..),
                ) => {}
                (
                    lexical::Kind::Scalar { style, empty },
                    TokenType::Scalar(original_style, value),
                ) => {
                    assert_eq!(style, original_style);
                    assert_eq!(empty, value.is_empty());
                }
                (
                    lexical::Kind::Comment { text, placement },
                    TokenType::Comment(comment),
                ) => {
                    assert_eq!(placement, comment.placement());
                    assert_eq!(text, comment.text());
                }
                (_, token) => panic!("incorrect stored kind for {token:?}"),
            }
            assert_span_eq(span, original);
            zero_width += usize::from(span.start == span.end);
        }
        assert!(reconstructed.next().is_none());
        for ((start, end, kind), (span, full)) in
            tokens.bytes().zip(tokens.iter(source))
        {
            assert_eq!(
                (Some(start), Some(end)),
                (span.start.byte_offset(), span.end.byte_offset())
            );
            assert_eq!(std::mem::discriminant(&kind), std::mem::discriminant(&full));
        }
        assert!(matches!(
            tokens.iter(source).next().unwrap().1,
            lexical::Kind::StreamStart
        ));
        assert_eq!(
            tokens.iter(source).skip(1).count() + 1,
            tokens.iter(source).count()
        );
    }
    assert!(zero_width > 10);
}

#[test]
fn compact_spans_preserve_absent_byte_offsets_and_indent() {
    for indent in [None, Some(0), Some(17)] {
        let span =
            Span::new(Marker::new(0, 1, 0), Marker::new(3, 2, 1)).with_indent(indent);
        let mut spans = spans::Spans::default();
        let stored = spans.store(&span);
        assert_span_eq(spans.get(stored), span);
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
            let mut spans = spans::Spans::default();
            let stored = spans.store(&span);
            assert_span_eq(spans.get(stored), span);
            let mut tokens = lexical::Tokens::default();
            tokens.push(&span, ryl_yaml_parser::TokenKind::Key);
            assert_span_eq(tokens.iter("").next().unwrap().0, span);
            let byte = |marker: Marker| marker.byte_offset().unwrap_or(usize::MAX);
            let (start, end, _) = tokens.bytes().next().unwrap();
            assert_eq!((start, end), (byte(span.start), byte(span.end)));
        }
    }
}

#[test]
fn borrowed_scalar_outside_source_survives_reconstruction() {
    let event = Event::Scalar(Cow::Borrowed("outside"), ScalarStyle::Plain, 0, None);
    let mut tables = events::Tables::default();
    let stored = events::StoredEvent::new("input", event.clone(), &mut tables);
    assert_eq!(stored.borrow("input", &tables), event);
}

#[test]
fn compact_records_are_smaller_than_parser_records() {
    assert_eq!(size_of::<spans::StoredSpan>(), 32);
    assert!(size_of::<(events::StoredEvent, spans::StoredSpan)>() <= 56);
    assert!(size_of::<events::StoredEvent>() < size_of::<Event<'_>>());
}

#[test]
fn stored_spans_keep_earlier_positions_exact_after_unicode() {
    let mut tokens = spans::Spans::default();
    let mut events = spans::Spans::default();
    let before = Span::empty(Marker::new(1, 1, 1).with_byte_offset(Some(1)));
    let after = Span::empty(Marker::new(2, 1, 2).with_byte_offset(Some(3)));
    let bom = Span::empty(Marker::new(1, 1, 0).with_byte_offset(Some(3)));
    let first = tokens.store(&before);
    let second = events.store(&after);
    let third = events.store(&bom);
    assert_span_eq(tokens.get(first), before);
    assert_span_eq(events.get(second), after);
    assert_span_eq(events.get(third), bom);
}

#[test]
fn inconsistent_marker_coordinates_preserve_explicit_positions() {
    let mut spans = spans::Spans::default();
    for marker in [
        Marker::new(0, 0, 0).with_byte_offset(Some(0)),
        Marker::new(0, u32::MAX as usize, 0).with_byte_offset(Some(0)),
        Marker::new(0, 2, 0).with_byte_offset(Some(0)),
        Marker::new(0, 1, 1).with_byte_offset(Some(0)),
        Marker::new(3, 1, 0).with_byte_offset(Some(0)),
    ] {
        let span = Span::empty(marker);
        let stored = spans.store(&span);
        assert_span_eq(spans.get(stored), span);
    }
    let span = Span::new(
        Marker::new(0, 1, 0).with_byte_offset(Some(0)),
        Marker::new(0, 1 << 31, 0).with_byte_offset(Some(0)),
    );
    let stored = spans.store(&span);
    assert_span_eq(spans.get(stored), span);
}

#[test]
fn sparse_lines_preserve_event_and_token_positions() {
    let source = format!("{}🦀: ['é', 世] # fin\r\n", "\n".repeat(8192));
    let mut stored = spans::Spans::default();
    let mut tokens = lexical::Tokens::default();
    let parser = Parser::with_options_and_observer(
        StrInput::new(&source),
        ryl_yaml_parser::options! { strict_indentation: true },
        |span: &Span, kind: ryl_yaml_parser::TokenKind<'_>| tokens.push(span, kind),
    );
    for item in parser {
        let (_, span) = item.unwrap();
        let compact = stored.store(&span);
        assert_span_eq(stored.get(compact), span);
    }
    let original = Scanner::new(StrInput::new(&source)).map(|item| item.unwrap());
    for ((span, _), token) in tokens.iter(&source).zip(original) {
        assert_span_eq(span, token.span());
    }
}
