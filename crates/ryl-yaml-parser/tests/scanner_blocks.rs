//! Coverage tests for block scalars, quoted scalars, plain scalars and error paths in
//! `src/scanner.rs`.
//!
//! Some tests use custom [`Input`] implementations (a legitimate use of the public `Input` /
//! `BorrowedInput` traits) to exercise code paths that `StrInput` and `BufferedInput` cannot
//! reach:
//!
//! - [`WindowedInput`] models a streaming input whose lookahead window drains as characters are
//!   consumed, exercising the default block-scalar line reader with a shrinking window.
//! - [`SliceableStreamInput`] models an input with stable byte offsets (`byte_offset` /
//!   `slice_bytes`) but without zero-copy borrowing (`slice_borrowed` returns `None`), which
//!   forces the owned-copy fallbacks when finalizing quoted scalars.

use granit_parser::{
    input::{is_breakz, BorrowedInput, Input},
    Event, Parser, ScalarStyle, ScanError, StructureStyle,
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

fn scalars_of(events: &[Event<'_>]) -> Vec<(String, ScalarStyle)> {
    events
        .iter()
        .filter_map(|event| {
            if let Event::Scalar(value, style, ..) = event {
                Some((value.to_string(), *style))
            } else {
                None
            }
        })
        .collect()
}

// -------------------------------------------------------------------------------------------
// Custom inputs
// -------------------------------------------------------------------------------------------

/// Maximum lookahead window of [`WindowedInput`].
const WINDOW: usize = 8;

/// A streaming input whose lookahead window shrinks as characters are consumed.
///
/// Unlike `BufferedInput` (which keeps its buffer topped up after every consumption), this input
/// only promises the characters requested by the latest `lookahead` call. Consuming characters
/// drains the window, so `buf_is_empty` eventually becomes `true` mid-line.
struct WindowedInput {
    chars: Vec<char>,
    pos: usize,
    window: usize,
}

impl WindowedInput {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            window: 0,
        }
    }

    fn consume_one(&mut self) {
        if self.pos < self.chars.len() {
            self.pos += 1;
        }
        self.window = self.window.saturating_sub(1);
    }
}

impl Input for WindowedInput {
    fn lookahead(&mut self, count: usize) {
        self.window = self.window.max(count.min(WINDOW));
    }

    fn buflen(&self) -> usize {
        self.window
    }

    fn bufmaxlen(&self) -> usize {
        WINDOW
    }

    fn raw_read_ch(&mut self) -> char {
        let c = self.chars.get(self.pos).copied().unwrap_or('\0');
        self.consume_one();
        c
    }

    fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied()?;
        if is_breakz(c) {
            None
        } else {
            self.consume_one();
            Some(c)
        }
    }

    fn skip(&mut self) {
        self.consume_one();
    }

    fn skip_n(&mut self, count: usize) {
        for _ in 0..count {
            self.consume_one();
        }
    }

    fn peek(&self) -> char {
        self.chars.get(self.pos).copied().unwrap_or('\0')
    }

    fn peek_nth(&self, n: usize) -> char {
        self.chars.get(self.pos + n).copied().unwrap_or('\0')
    }
}

impl BorrowedInput<'static> for WindowedInput {
    fn slice_borrowed(&self, _start: usize, _end: usize) -> Option<&'static str> {
        None
    }
}

/// An input with stable byte offsets and (optionally) `slice_bytes`, but no zero-copy borrowing.
///
/// This models an input that owns its backing storage: it can hand out `&str` slices tied to
/// `&self` (`slice_bytes`) but not slices with the `'input` lifetime (`slice_borrowed`).
struct SliceableStreamInput {
    source: String,
    /// Current byte offset into `source`.
    pos: usize,
    /// Sticky lookahead window, mirroring `StrInput`.
    window: usize,
    /// Whether `slice_bytes` is offered.
    provide_slices: bool,
}

impl SliceableStreamInput {
    fn new(source: &str, provide_slices: bool) -> Self {
        Self {
            source: source.to_owned(),
            pos: 0,
            window: 0,
            provide_slices,
        }
    }

    fn rest(&self) -> &str {
        &self.source[self.pos..]
    }
}

impl Input for SliceableStreamInput {
    fn lookahead(&mut self, count: usize) {
        self.window = self.window.max(count);
    }

    fn buflen(&self) -> usize {
        self.window
    }

    fn bufmaxlen(&self) -> usize {
        128
    }

    fn raw_read_ch(&mut self) -> char {
        match self.rest().chars().next() {
            Some(c) => {
                self.pos += c.len_utf8();
                c
            }
            None => '\0',
        }
    }

    fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
        let c = self.rest().chars().next()?;
        if is_breakz(c) {
            None
        } else {
            self.pos += c.len_utf8();
            Some(c)
        }
    }

    fn skip(&mut self) {
        if let Some(c) = self.rest().chars().next() {
            self.pos += c.len_utf8();
        }
    }

    fn skip_n(&mut self, count: usize) {
        for _ in 0..count {
            self.skip();
        }
    }

    fn peek(&self) -> char {
        self.rest().chars().next().unwrap_or('\0')
    }

    fn peek_nth(&self, n: usize) -> char {
        self.rest().chars().nth(n).unwrap_or('\0')
    }

    fn byte_offset(&self) -> Option<usize> {
        Some(self.pos)
    }

    fn slice_bytes(&self, start: usize, end: usize) -> Option<&str> {
        if self.provide_slices {
            self.source.get(start..end)
        } else {
            None
        }
    }
}

impl BorrowedInput<'static> for SliceableStreamInput {
    fn slice_borrowed(&self, _start: usize, _end: usize) -> Option<&'static str> {
        None
    }
}

// -------------------------------------------------------------------------------------------
// Block scalars
// -------------------------------------------------------------------------------------------

/// The default block-scalar line reader must consume the full line even when the input's
/// lookahead window drains partway through it.
#[test]
fn block_scalar_line_longer_than_lookahead_window_is_read_completely() {
    let events: Result<Vec<Event<'static>>, ScanError> = Parser::new(WindowedInput::new(
        "|\n abcdefghijklmnopqrstuvwxyz 0123456789\n",
    ))
    .map(|event| event.map(|(event, _)| event))
    .collect();
    let events = events.expect("valid literal scalar must parse");

    assert_eq!(
        scalars_of(&events),
        vec![(
            "abcdefghijklmnopqrstuvwxyz 0123456789\n".to_owned(),
            ScalarStyle::Literal
        )]
    );
}

/// A folded scalar with two long lines exercises the default line reader together with line
/// folding.
#[test]
fn folded_scalar_long_lines_with_windowed_input_fold_to_spaces() {
    let events: Result<Vec<Event<'static>>, ScanError> = Parser::new(WindowedInput::new(
        ">\n the quick brown fox jumps over\n the lazy dog and runs away\n",
    ))
    .map(|event| event.map(|(event, _)| event))
    .collect();
    let events = events.expect("valid folded scalar must parse");

    assert_eq!(
        scalars_of(&events),
        vec![(
            "the quick brown fox jumps over the lazy dog and runs away\n".to_owned(),
            ScalarStyle::Folded
        )]
    );
}

/// A block scalar indented deeper than the input's buffer size must skip indentation through the
/// chunked loop (scanner.rs `skip_block_scalar_indent`, `indent >= bufmaxlen - 2` branch),
/// including its early exit when a line holds content before the indent level is reached.
#[test]
fn deeply_indented_block_scalar_with_buffered_input() {
    let indent = " ".repeat(16);
    let yaml = format!("k:\n{indent}|\n{indent}aaaa\n\n{indent}bbbb\n");

    let events: Result<Vec<Event<'static>>, ScanError> =
        Parser::new_from_iter(yaml.chars().collect::<Vec<char>>().into_iter())
            .map(|event| event.map(|(event, _)| event))
            .collect();
    let events = events.expect("valid literal scalar must parse");

    assert_eq!(
        scalars_of(&events),
        vec![
            ("k".to_owned(), ScalarStyle::Plain),
            ("aaaa\n\nbbbb\n".to_owned(), ScalarStyle::Literal),
        ]
    );
}

/// A block scalar whose indentation is deeper than the input's lookahead buffer must re-request
/// lookahead in the middle of skipping a single line's indentation (scanner.rs
/// `skip_block_scalar_indent`, inner loop repeating because the buffer drained before the indent
/// level was reached).
#[test]
fn deeply_indented_block_scalar_with_windowed_input() {
    let indent = " ".repeat(10);
    let yaml = format!("k:\n{indent}|\n{indent}aaaa\n\n{indent}bbbb\n");

    let events: Result<Vec<Event<'static>>, ScanError> = Parser::new(WindowedInput::new(&yaml))
        .map(|event| event.map(|(event, _)| event))
        .collect();
    let events = events.expect("valid literal scalar must parse");

    assert_eq!(
        scalars_of(&events),
        vec![
            ("k".to_owned(), ScalarStyle::Plain),
            ("aaaa\n\nbbbb\n".to_owned(), ScalarStyle::Literal),
        ]
    );
}

// -------------------------------------------------------------------------------------------
// Quoted scalars
// -------------------------------------------------------------------------------------------

/// Trailing blanks before a line break inside a double-quoted scalar are discarded; with
/// `StrInput` this forces the zero-copy buffer to be promoted to an owned string
/// (scanner.rs `scan_flow_scalar`, pending-whitespace discard branch).
#[test]
fn double_quoted_trailing_blank_before_break_folds_to_space() {
    let events = parse_events("\"a \nb\"\n").unwrap();

    assert_eq!(
        scalars_of(&events),
        vec![("a b".to_owned(), ScalarStyle::DoubleQuoted)]
    );
}

/// Same as above for single-quoted scalars, with several blanks before the break.
#[test]
fn single_quoted_trailing_blanks_before_break_fold_to_space() {
    let events = parse_events("'a  \n  b'\n").unwrap();

    assert_eq!(
        scalars_of(&events),
        vec![("a b".to_owned(), ScalarStyle::SingleQuoted)]
    );
}

/// An input that advertises byte offsets but no zero-copy borrowing makes the scanner fall back
/// to copying the quoted scalar out of `slice_bytes` (scanner.rs `scan_flow_scalar`, borrowed
/// contents fallback).
#[test]
fn quoted_scalar_from_offsets_without_borrowing_is_copied() {
    let events: Result<Vec<Event<'static>>, ScanError> =
        Parser::new(SliceableStreamInput::new("\"hello world\"\n", true))
            .map(|event| event.map(|(event, _)| event))
            .collect();
    let events = events.expect("valid double quoted scalar must parse");

    assert_eq!(
        scalars_of(&events),
        vec![("hello world".to_owned(), ScalarStyle::DoubleQuoted)]
    );
}

/// If the input advertises byte offsets but then refuses to provide the slice, the scanner
/// reports an internal error rather than panicking.
#[test]
fn quoted_scalar_from_offsets_without_slices_is_internal_error() {
    let mut parser = Parser::new(SliceableStreamInput::new("\"hello world\"\n", false));

    let error = parser.find_map(Result::err).expect("expected parser error");
    assert_eq!(
        error.info(),
        "internal error: input advertised offsets but did not provide a slice"
    );
}

// -------------------------------------------------------------------------------------------
// Keys and error paths
// -------------------------------------------------------------------------------------------

/// An explicit key indicator right after a flow sequence on the same line is invalid
/// (scanner.rs `fetch_key`, "mapping keys are not allowed in this context").
#[test]
fn explicit_key_after_flow_sequence_end_is_rejected() {
    assert_eq!(
        first_error_info("[a] ? b\n"),
        "mapping keys are not allowed in this context"
    );
}

/// A `,` after a flow sequence that opened at a required-simple-key position trips the
/// required-key check in `remove_simple_key` (scanner.rs, "simple key expected ':'").
#[test]
fn flow_entry_after_required_simple_key_is_rejected() {
    assert_eq!(
        first_error_info("x: y\n[a], b\n"),
        "simple key expected ':'"
    );
}

/// An empty flow sequence used as a mapping key in an already-open block mapping calls
/// `roll_indent` while the current indent is still the one-column indent prepared by the `[`
/// indicator, so no new block mapping is started (scanner.rs `roll_indent`,
/// `self.indent > col` case).
#[test]
fn empty_flow_sequence_key_in_existing_block_mapping() {
    let events = parse_events("x: y\n[]: b\n").unwrap();

    assert_eq!(
        events,
        vec![
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar("x".into(), ScalarStyle::Plain, 0, None),
            Event::Scalar("y".into(), ScalarStyle::Plain, 0, None),
            Event::SequenceStart(StructureStyle::Flow, 0, None),
            Event::SequenceEnd,
            Event::Scalar("b".into(), ScalarStyle::Plain, 0, None),
            Event::MappingEnd,
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

/// A block sequence indented one column under its mapping key replaces the non-block indent
/// prepared by the `:` value with a block sequence indent (scanner.rs `roll_indent`, non-block
/// indent removal).
#[test]
fn sequence_indented_one_column_under_mapping_key() {
    let events = parse_events("a:\n - b\n").unwrap();

    assert_eq!(
        events,
        vec![
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar("a".into(), ScalarStyle::Plain, 0, None),
            Event::SequenceStart(StructureStyle::Block, 0, None),
            Event::Scalar("b".into(), ScalarStyle::Plain, 0, None),
            Event::SequenceEnd,
            Event::MappingEnd,
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}
