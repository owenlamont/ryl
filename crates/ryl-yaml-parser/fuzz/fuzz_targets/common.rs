#![allow(dead_code)]

use std::collections::BTreeSet;

use granit_parser::{
    BufferedInput, ErrorKind, Event, FallibleBufferedInput, Marker, Options, Parser, ScanError,
    Scanner, Span, StrInput, Token, TokenType,
};

#[derive(Debug)]
struct Trace<T> {
    items: Vec<T>,
    error: Option<ScanError>,
}

/// Parse with the default options and require every input backend to agree.
pub fn parse_with_both_inputs(input: &str) {
    parse_with_options(input, Options::default());
}

/// Parse with string, buffered, and fallible-buffered inputs and compare their full traces.
///
/// Invalid YAML is useful fuzz input. Consequently, this retains and compares both the events
/// emitted before the first error and the error itself instead of discarding either one.
pub fn parse_with_options(input: &str, options: Options) {
    let string = trace(Parser::new_from_str_with_options(input, options.clone()));
    let buffered = trace(Parser::new_from_iter_with_options(
        input.chars(),
        options.clone(),
    ));
    let fallible = trace(Parser::new_from_fallible_iter_with_options(
        input.chars().map(Ok::<char, ErrorKind>),
        options,
    ));

    validate_string_parser_trace(input, &string);
    validate_streaming_parser_trace(&buffered);
    validate_streaming_parser_trace(&fallible);

    assert_eq!(
        string.items, buffered.items,
        "StrInput and BufferedInput emitted different parser events"
    );
    assert_eq!(
        string.error, buffered.error,
        "StrInput and BufferedInput returned different parser errors"
    );
    assert_eq!(
        string.items, fallible.items,
        "infallible and all-Ok fallible inputs emitted different parser events"
    );
    assert_eq!(
        string.error, fallible.error,
        "infallible and all-Ok fallible inputs returned different parser errors"
    );
}

/// Scan with the default options and require every input backend to agree.
pub fn scan_with_all_inputs(input: &str) {
    scan_with_options(input, Options::default());
}

/// Scan with string, buffered, and fallible-buffered inputs and compare their full traces.
pub fn scan_with_options(input: &str, options: Options) {
    let string = trace(Scanner::with_options(StrInput::new(input), options.clone()));
    let buffered = trace(Scanner::with_options(
        BufferedInput::new(input.chars()),
        options.clone(),
    ));
    let fallible = trace(Scanner::with_options(
        FallibleBufferedInput::new(input.chars().map(Ok::<char, ErrorKind>)),
        options,
    ));

    validate_string_scanner_trace(input, &string);
    validate_streaming_scanner_trace(&buffered);
    validate_streaming_scanner_trace(&fallible);

    assert_eq!(
        string.items, buffered.items,
        "StrInput and BufferedInput emitted different scanner tokens"
    );
    assert_eq!(
        string.error, buffered.error,
        "StrInput and BufferedInput returned different scanner errors"
    );
    assert_eq!(
        string.items, fallible.items,
        "infallible and all-Ok fallible inputs emitted different scanner tokens"
    );
    assert_eq!(
        string.error, fallible.error,
        "infallible and all-Ok fallible inputs returned different scanner errors"
    );
}

/// Disabling comments must only remove comment events and tokens.
pub fn check_comment_suppression(input: &str) {
    check_comment_suppression_with_options(input, Options::default());
}

/// Comment suppression must preserve the trace and errors under the selected parsing options.
pub fn check_comment_suppression_with_options(input: &str, mut options: Options) {
    // Prevent the enabled parser from failing at the buffered-comment resource limit. That limit
    // intentionally does not apply when comment emission is disabled.
    #[cfg(feature = "comments")]
    {
        options.max_buffered_comment_events = usize::MAX;
    }
    options.emit_comments = true;
    let with_comments = options.clone();
    options.emit_comments = false;
    let without_comments = options;

    let parser_with = trace(Parser::new_from_str_with_options(
        input,
        with_comments.clone(),
    ));
    let parser_without = trace(Parser::new_from_str_with_options(
        input,
        without_comments.clone(),
    ));
    validate_string_parser_trace(input, &parser_with);
    validate_string_parser_trace(input, &parser_without);

    let parser_non_comments: Vec<_> = parser_with
        .items
        .into_iter()
        .filter(|(event, _)| !is_comment_event(event))
        .collect();
    assert_eq!(
        parser_non_comments, parser_without.items,
        "comment suppression changed non-comment parser events"
    );
    assert_eq!(
        parser_with.error, parser_without.error,
        "comment suppression changed the parser error"
    );

    let scanner_with = trace(Scanner::with_options(StrInput::new(input), with_comments));
    let scanner_without = trace(Scanner::with_options(
        StrInput::new(input),
        without_comments,
    ));
    validate_string_scanner_trace(input, &scanner_with);
    validate_string_scanner_trace(input, &scanner_without);

    let scanner_non_comments: Vec<_> = scanner_with
        .items
        .into_iter()
        .filter(|token| !is_comment_token(token.token_type()))
        .collect();
    assert_eq!(
        scanner_non_comments, scanner_without.items,
        "comment suppression changed non-comment scanner tokens"
    );
    assert_eq!(
        scanner_with.error, scanner_without.error,
        "comment suppression changed the scanner error"
    );
}

fn trace<T>(mut iterator: impl Iterator<Item = Result<T, ScanError>>) -> Trace<T> {
    let mut items = Vec::new();

    loop {
        match iterator.next() {
            Some(Ok(item)) => items.push(item),
            Some(Err(error)) => {
                assert!(
                    iterator.next().is_none() && iterator.next().is_none(),
                    "iterator did not fuse after an error"
                );
                return Trace {
                    items,
                    error: Some(error),
                };
            }
            None => {
                assert!(
                    iterator.next().is_none() && iterator.next().is_none(),
                    "iterator did not fuse after exhaustion"
                );
                return Trace { items, error: None };
            }
        }
    }
}

fn validate_string_parser_trace(input: &str, trace: &Trace<(Event<'_>, Span)>) {
    let byte_offsets = byte_offsets(input);
    for (_, span) in &trace.items {
        validate_string_span(input, &byte_offsets, *span);
    }
    if let Some(error) = &trace.error {
        validate_string_marker(input, &byte_offsets, *error.marker());
    }
    validate_parser_structure(&trace.items, trace.error.is_none());
}

fn validate_streaming_parser_trace(trace: &Trace<(Event<'_>, Span)>) {
    for (_, span) in &trace.items {
        validate_streaming_span(*span);
    }
    if let Some(error) = &trace.error {
        assert_eq!(error.marker().byte_offset(), None);
    }
    validate_parser_structure(&trace.items, trace.error.is_none());
}

fn validate_string_scanner_trace(input: &str, trace: &Trace<Token<'_>>) {
    let byte_offsets = byte_offsets(input);
    for token in &trace.items {
        validate_string_span(input, &byte_offsets, token.span());
    }
    if let Some(error) = &trace.error {
        validate_string_marker(input, &byte_offsets, *error.marker());
    }
    validate_scanner_bounds(&trace.items, trace.error.is_none());
}

fn validate_streaming_scanner_trace(trace: &Trace<Token<'_>>) {
    for token in &trace.items {
        validate_streaming_span(token.span());
    }
    if let Some(error) = &trace.error {
        assert_eq!(error.marker().byte_offset(), None);
    }
    validate_scanner_bounds(&trace.items, trace.error.is_none());
}

fn byte_offsets(input: &str) -> Vec<usize> {
    let mut offsets: Vec<_> = input.char_indices().map(|(offset, _)| offset).collect();
    offsets.push(input.len());
    offsets
}

fn validate_string_span(input: &str, byte_offsets: &[usize], span: Span) {
    assert!(
        span.start.index() <= span.end.index(),
        "span ends before it starts: {span:?}"
    );
    validate_string_marker(input, byte_offsets, span.start);
    validate_string_marker(input, byte_offsets, span.end);
    if let Some(tag_start) = span.tag_start() {
        validate_string_marker(input, byte_offsets, tag_start);
    }
    assert!(
        span.slice(input).is_some(),
        "span does not slice its source: {span:?}"
    );
}

fn validate_string_marker(input: &str, byte_offsets: &[usize], marker: Marker) {
    let expected = byte_offsets
        .get(marker.index())
        .copied()
        .expect("marker character index lies past the source");
    assert_eq!(
        marker.byte_offset(),
        Some(expected),
        "marker byte and character offsets disagree"
    );
    assert!(input.is_char_boundary(expected));
}

fn validate_streaming_span(span: Span) {
    assert!(
        span.start.index() <= span.end.index(),
        "span ends before it starts: {span:?}"
    );
    assert_eq!(span.start.byte_offset(), None);
    assert_eq!(span.end.byte_offset(), None);
    if let Some(tag_start) = span.tag_start() {
        assert_eq!(tag_start.byte_offset(), None);
    }
}

fn validate_parser_structure(events: &[(Event<'_>, Span)], complete: bool) {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Collection {
        Mapping,
        Sequence,
    }

    if complete || !events.is_empty() {
        assert!(
            matches!(events.first(), Some((Event::StreamStart, _))),
            "parser trace does not start with StreamStart"
        );
    }
    if complete {
        assert!(
            matches!(events.last(), Some((Event::StreamEnd, _))),
            "successful parser trace does not end with StreamEnd"
        );
    }

    let mut document_open = false;
    let mut collections = Vec::new();
    let mut anchors = BTreeSet::new();

    for (index, (event, _)) in events.iter().enumerate() {
        match event {
            Event::StreamStart => assert_eq!(index, 0, "duplicate StreamStart event"),
            Event::StreamEnd => {
                assert!(complete, "StreamEnd preceded a parser error");
                assert_eq!(index + 1, events.len(), "early StreamEnd event");
                assert!(!document_open, "StreamEnd occurred inside a document");
                assert!(
                    collections.is_empty(),
                    "StreamEnd occurred inside a collection"
                );
            }
            Event::DocumentStart(..) => {
                assert!(!document_open, "nested DocumentStart event");
                assert_eq!(collections, Vec::<Collection>::new());
                document_open = true;
                anchors.clear();
            }
            Event::DocumentEnd => {
                assert!(document_open, "DocumentEnd without DocumentStart");
                assert!(collections.is_empty(), "document ended inside a collection");
                document_open = false;
            }
            Event::SequenceStart(_, anchor, _) => {
                assert!(document_open, "sequence outside a document");
                remember_anchor(&mut anchors, *anchor);
                collections.push(Collection::Sequence);
            }
            Event::SequenceEnd => {
                assert_eq!(collections.pop(), Some(Collection::Sequence));
            }
            Event::MappingStart(_, anchor, _) => {
                assert!(document_open, "mapping outside a document");
                remember_anchor(&mut anchors, *anchor);
                collections.push(Collection::Mapping);
            }
            Event::MappingEnd => {
                assert_eq!(collections.pop(), Some(Collection::Mapping));
            }
            Event::Scalar(_, _, anchor, _) => {
                assert!(document_open, "scalar outside a document");
                remember_anchor(&mut anchors, *anchor);
            }
            Event::Alias(anchor) => {
                assert!(document_open, "alias outside a document");
                assert!(
                    anchors.contains(anchor),
                    "alias refers to an anchor not yet defined in this document"
                );
            }
            _ => {}
        }
    }

    // An error may leave documents and collections open, but all events emitted before it
    // must still form a valid prefix (including anchor definitions preceding aliases).
    if complete {
        assert!(!document_open, "parser trace ended inside a document");
        assert!(
            collections.is_empty(),
            "parser trace ended inside a collection"
        );
    }
}

fn remember_anchor(anchors: &mut BTreeSet<usize>, anchor: usize) {
    if anchor != 0 {
        assert!(
            anchors.insert(anchor),
            "anchor ID was emitted more than once"
        );
    }
}

fn validate_scanner_bounds(tokens: &[Token<'_>], complete: bool) {
    if complete || !tokens.is_empty() {
        assert!(
            matches!(
                tokens.first().map(Token::token_type),
                Some(TokenType::StreamStart)
            ),
            "scanner trace does not start with StreamStart"
        );
    }
    if complete {
        assert!(
            matches!(
                tokens.last().map(Token::token_type),
                Some(TokenType::StreamEnd)
            ),
            "successful scanner trace does not end with StreamEnd"
        );
    }
    assert_eq!(
        tokens
            .iter()
            .filter(|token| matches!(token.token_type(), TokenType::StreamStart))
            .count(),
        usize::from(!tokens.is_empty()),
        "scanner emitted multiple StreamStart tokens"
    );
    assert_eq!(
        tokens
            .iter()
            .filter(|token| matches!(token.token_type(), TokenType::StreamEnd))
            .count(),
        usize::from(complete),
        "scanner must emit StreamEnd exactly once on success and never before an error"
    );
}

pub fn is_comment_event(event: &Event<'_>) -> bool {
    #[cfg(feature = "comments")]
    {
        matches!(event, Event::Comment(..))
    }
    #[cfg(not(feature = "comments"))]
    {
        let _ = event;
        false
    }
}

pub fn is_comment_token(token: &TokenType<'_>) -> bool {
    #[cfg(feature = "comments")]
    {
        matches!(token, TokenType::Comment(_))
    }
    #[cfg(not(feature = "comments"))]
    {
        let _ = token;
        false
    }
}

pub fn comment_modes() -> &'static [bool] {
    #[cfg(feature = "comments")]
    {
        &[true, false]
    }
    #[cfg(not(feature = "comments"))]
    {
        &[false]
    }
}
