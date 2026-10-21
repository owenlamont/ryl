//! Coverage tests for rarely-exercised parser and input paths.
//!
//! These tests target scattered error paths and default trait implementations that the
//! regular test suite does not reach.

#[cfg(feature = "comments")]
use granit_parser::Placement;

use granit_parser::{
    input::{SkipTabs, WhitespaceResult},
    BufferedInput, ErrorKind, Event, Input, Parser, ScanError, StrInput, TryEventReceiver,
    TryLoadError,
};

fn parse_events(input: &str) -> Result<Vec<Event<'_>>, ScanError> {
    Parser::new_from_str(input)
        .map(|event| event.map(|(event, _)| event))
        .collect()
}

fn first_error_info(input: &str) -> String {
    for event in Parser::new_from_str(input) {
        if let Err(error) = event {
            return error.info();
        }
    }
    panic!("expected parser error");
}

/// A receiver that rejects the terminal `StreamEnd` event.
struct RejectStreamEnd;

impl<'input> TryEventReceiver<'input> for RejectStreamEnd {
    type Error = &'static str;

    fn on_event(&mut self, ev: Event<'input>) -> Result<(), Self::Error> {
        if matches!(ev, Event::StreamEnd) {
            Err("stream end rejected")
        } else {
            Ok(())
        }
    }
}

/// A receiver that accepts everything.
struct AcceptAll;

impl<'input> TryEventReceiver<'input> for AcceptAll {
    type Error = &'static str;

    fn on_event(&mut self, _ev: Event<'input>) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// A receiver used to prove that a buffered structural event precedes a deferred scanner error.
struct RejectSequenceStart;

impl<'input> TryEventReceiver<'input> for RejectSequenceStart {
    type Error = &'static str;

    fn on_event(&mut self, ev: Event<'input>) -> Result<(), Self::Error> {
        if matches!(ev, Event::SequenceStart(..)) {
            Err("sequence start reached")
        } else {
            Ok(())
        }
    }
}

// --- input.rs: default `Input::skip_ws_to_eol` (only reachable through `BufferedInput`,
// --- since `StrInput` overrides it and the scanner uses `skip_ws_to_eol_blanks` whenever
// --- comments are possible).

#[test]
fn buffered_default_skip_ws_to_eol_consumes_blanks_and_comment() {
    let mut input = BufferedInput::new("  \t # note\nx".chars());

    let (consumed, result) = input.skip_ws_to_eol(SkipTabs::Yes);

    // 2 spaces + 1 tab + 1 space + '#' + " note" (5 chars) = 10 characters.
    assert_eq!(consumed, 10);
    let skipped = result.expect("whitespace with a comment must be accepted");
    assert_eq!(skipped, WhitespaceResult::new(true, true));
    // The line break must not be consumed.
    assert_eq!(input.look_ch(), '\n');
}

#[test]
fn buffered_default_skip_ws_to_eol_rejects_comment_without_whitespace() {
    let mut input = BufferedInput::new("#no-space".chars());

    let (consumed, result) = input.skip_ws_to_eol(SkipTabs::Yes);

    assert_eq!(consumed, 0);
    let Err(kind) = result else {
        panic!("expected an error for a comment without leading whitespace");
    };
    assert_eq!(kind, ErrorKind::CommentNotSeparated);
    // The '#' itself must not be consumed.
    assert_eq!(input.look_ch(), '#');
}

#[test]
fn buffered_default_skip_ws_to_eol_stops_at_tab_when_tabs_disallowed() {
    let mut input = BufferedInput::new("\tx".chars());

    let (consumed, result) = input.skip_ws_to_eol(SkipTabs::No);

    assert_eq!(consumed, 0);
    let skipped = result.expect("stopping at a tab is not an error");
    assert_eq!(skipped, WhitespaceResult::new(false, false));
    assert_eq!(input.look_ch(), '\t');
}

// --- input/buffered.rs

#[test]
fn buffered_raw_read_ch_pads_after_source_is_exhausted() {
    let mut input = BufferedInput::new("a".chars());

    assert_eq!(input.raw_read_ch(), 'a');
    // First EOF read marks the source as exhausted...
    assert_eq!(input.raw_read_ch(), '\0');
    // ...and subsequent reads keep returning the EOF padding character.
    assert_eq!(input.raw_read_ch(), '\0');
}

#[test]
fn buffered_raw_read_non_breakz_ch_uses_buffered_front() {
    let mut input = BufferedInput::new("a\nb".chars());

    // Fill the buffer so `raw_read_non_breakz_ch` takes the buffered-front path.
    input.lookahead(2);
    assert_eq!(input.raw_read_non_breakz_ch(), Some('a'));
    // The buffered front is now the line break: it must be left in place.
    assert_eq!(input.raw_read_non_breakz_ch(), None);
    assert_eq!(input.peek(), '\n');
}

// --- input/str.rs: `skip_ws_to_eol_blanks` with `SkipTabs::No`

#[test]
fn str_input_skip_ws_to_eol_blanks_stops_before_tab_when_tabs_disallowed() {
    let mut input = StrInput::new("  \tfoo");

    let (consumed, skipped) = input.skip_ws_to_eol_blanks(SkipTabs::No);

    assert_eq!(consumed, 2);
    assert_eq!(skipped, WhitespaceResult::new(false, true));
    assert_eq!(input.look_ch(), '\t');
}

// --- parser.rs: `parse_node` error reporting per state

#[test]
fn stray_flow_entry_in_block_sequence_reports_block_sequence_error() {
    // `,` is tokenized as a flow entry even outside a flow collection; `parse_node`
    // then rejects it while in `BlockNode` state.
    assert_eq!(
        first_error_info("- ,\n"),
        "unexpected EOF while parsing a block sequence"
    );
}

#[test]
fn stray_flow_entry_in_block_mapping_value_reports_block_mapping_error() {
    assert_eq!(
        first_error_info("a: ,\n"),
        "unexpected EOF while parsing a block mapping"
    );
}

// --- parser.rs: comment right after `:` in a flow-sequence explicit key/value pair
// --- (`FlowSequenceEntryMappingValue` state)

#[test]
fn comment_after_value_in_flow_sequence_explicit_pair_is_emitted() {
    let events = parse_events("[? a : # note\n b]\n").unwrap();

    #[cfg(not(feature = "comments"))]
    assert!(events.windows(3).any(|events| matches!(
        events,
        [Event::MappingStart(granit_parser::StructureStyle::Flow, 0, None), Event::Scalar(key, ..), Event::Scalar(value, ..)]
            if key == "a" && value == "b"
    )));
    #[cfg(feature = "comments")]
    {
        let comment_pos = events
            .iter()
            .position(|event| matches!(event, Event::Comment(text, _) if text == " note"))
            .expect("the comment between the key and value must be emitted");
        assert!(matches!(
            events[comment_pos - 2],
            Event::MappingStart(granit_parser::StructureStyle::Flow, 0, None)
        ));
        assert!(matches!(
            events[comment_pos - 1],
            Event::Scalar(ref value, ..) if value == "a"
        ));
        assert!(matches!(
            events[comment_pos + 1],
            Event::Scalar(ref value, ..) if value == "b"
        ));
        assert!(matches!(
            events[comment_pos],
            Event::Comment(_, Placement::Right)
        ));
    }
}

// --- parser.rs: `try_load` returning an error buffered by `peek`

#[test]
fn try_load_returns_error_buffered_by_peek() {
    let mut parser = Parser::new_from_str("a: *missing\n");

    // Drive the parser through `peek` until the unknown-alias error is buffered.
    let buffered_error = loop {
        match parser.peek() {
            Some(Ok(_)) => {
                parser.next_event().unwrap().unwrap();
            }
            Some(Err(error)) => break error,
            None => panic!("expected an unknown alias error"),
        }
    };
    assert_eq!(
        buffered_error.info(),
        "while parsing node, found unknown anchor"
    );

    let mut receiver = AcceptAll;
    let err = parser.try_load(&mut receiver, true).unwrap_err();
    assert_eq!(err, TryLoadError::Scan(buffered_error));

    // The buffered error is consumed; the parser is exhausted afterwards.
    assert!(parser.next_event().is_none());
}

#[test]
fn try_load_emits_peeked_sequence_start_before_deferred_scanner_error() {
    let mut parser = Parser::new_from_iter("a:\n-\nb".chars());

    // Drive through the common prefix. Peeking the sequence start also discovers and defers the
    // later scanner error on streaming inputs.
    loop {
        match parser.peek() {
            Some(Ok((Event::SequenceStart(..), _))) => break,
            Some(Ok(_)) => {
                parser.next_event().unwrap().unwrap();
            }
            Some(Err(error)) => panic!("sequence start was skipped: {error}"),
            None => panic!("expected an indentless sequence start"),
        }
    }

    let mut receiver = RejectSequenceStart;
    let error = parser.try_load(&mut receiver, true).unwrap_err();
    assert_eq!(error, TryLoadError::Receiver("sequence start reached"));

    let scan_error = parser.next_event().unwrap().unwrap_err();
    assert_eq!(scan_error.kind(), &ErrorKind::SimpleKeyExpected);
    assert!(parser.next_event().is_none());
}

// --- parser.rs: resuming after a receiver error on `StreamEnd`

#[test]
fn next_event_after_receiver_error_on_stream_end_returns_stream_end() {
    let mut parser = Parser::new_from_str("foo\n");
    let mut receiver = RejectStreamEnd;

    let err = parser.try_load(&mut receiver, true).unwrap_err();
    assert_eq!(err, TryLoadError::Receiver("stream end rejected"));

    // The parser already reached its terminal state; asking for another event
    // re-emits `StreamEnd` instead of scanning further.
    let (event, _) = parser.next_event().expect("stream end event").unwrap();
    assert_eq!(event, Event::StreamEnd);
    assert!(parser.next_event().is_none());
}

#[test]
fn next_event_after_receiver_error_on_stream_end_with_comments_returns_stream_end() {
    let mut parser = Parser::new_from_str("# hello\nfoo\n");
    let mut receiver = RejectStreamEnd;

    let err = parser.try_load(&mut receiver, true).unwrap_err();
    assert_eq!(err, TryLoadError::Receiver("stream end rejected"));

    let (event, _) = parser.next_event().expect("stream end event").unwrap();
    assert_eq!(event, Event::StreamEnd);
    assert!(parser.next_event().is_none());
}
