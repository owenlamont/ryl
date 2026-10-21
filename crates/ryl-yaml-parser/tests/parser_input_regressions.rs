use std::str::Chars;

use granit_parser::{
    BorrowedInput, BufferedInput, ErrorKind, Event, Input, Marker, Parser, ScanError, StrInput,
};

/// Models a third-party input whose optimized chunk hook still follows the 1.0.0 contract.
///
/// In 1.0.0 the hook may consume non-printable characters, so the scanner must retain a central
/// validation backstop even when its built-in inputs become more selective.
struct LegacyChunkInput<'a>(BufferedInput<Chars<'a>>);

impl<'a> LegacyChunkInput<'a> {
    fn new(source: &'a str) -> Self {
        Self(BufferedInput::new(source.chars()))
    }
}

impl Input for LegacyChunkInput<'_> {
    fn lookahead(&mut self, count: usize) {
        self.0.lookahead(count);
    }

    fn buflen(&self) -> usize {
        self.0.buflen()
    }

    fn bufmaxlen(&self) -> usize {
        self.0.bufmaxlen()
    }

    fn raw_read_ch(&mut self) -> char {
        self.0.raw_read_ch()
    }

    fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
        self.0.raw_read_non_breakz_ch()
    }

    fn skip(&mut self) {
        self.0.skip();
    }

    fn skip_n(&mut self, count: usize) {
        self.0.skip_n(count);
    }

    fn peek(&self) -> char {
        self.0.peek()
    }

    fn peek_nth(&self, n: usize) -> char {
        self.0.peek_nth(n)
    }

    fn fetch_plain_scalar_chunk(
        &mut self,
        out: &mut String,
        count: usize,
        flow_level_gt_0: bool,
    ) -> (bool, usize) {
        let mut chars_consumed = 0;
        for _ in 0..count {
            self.lookahead(1);
            if self.next_is_blank_or_breakz() || !self.next_can_be_plain_scalar(flow_level_gt_0) {
                return (true, chars_consumed);
            }
            out.push(self.peek());
            self.skip();
            chars_consumed += 1;
        }
        (false, chars_consumed)
    }
}

impl<'a> BorrowedInput<'a> for LegacyChunkInput<'_> {
    fn slice_borrowed(&self, _start: usize, _end: usize) -> Option<&'a str> {
        None
    }
}

fn first_str_error(input: &str) -> ScanError {
    Parser::new_from_str(input)
        .find_map(Result::err)
        .expect("input should fail")
}

fn first_iter_error(input: &str) -> ScanError {
    Parser::new_from_iter(input.chars())
        .find_map(Result::err)
        .expect("input should fail")
}

fn first_str_scalar(input: &str) -> String {
    Parser::new_from_str(input)
        .find_map(|item| match item.expect("input should parse") {
            (Event::Scalar(value, ..), _) => Some(value.into_owned()),
            _ => None,
        })
        .expect("input should contain a scalar")
}

fn first_iter_scalar(input: &str) -> String {
    Parser::new_from_iter(input.chars())
        .find_map(|item| match item.expect("input should parse") {
            (Event::Scalar(value, ..), _) => Some(value.into_owned()),
            _ => None,
        })
        .expect("input should contain a scalar")
}

#[test]
fn parser_iterator_terminates_after_scan_error() {
    let parser = Parser::new_from_str("foo:\n  bar\ninvalid\n");
    let mut errors = 0usize;
    let mut events = 0usize;

    for item in parser {
        events += 1;
        if item.is_err() {
            errors += 1;
        }
        assert!(
            events < 1000,
            "Parser iterator did not terminate after a scan error"
        );
    }

    assert_eq!(errors, 1, "error should be yielded exactly once");
}

#[test]
fn buffered_skip_n_matches_str_input_and_saturates_at_eof() {
    let mut buffered = BufferedInput::new("abc".chars());
    buffered.lookahead(1);
    buffered.skip_n(2);
    buffered.lookahead(1);

    let mut str_input = StrInput::new("abc");
    str_input.lookahead(1);
    str_input.skip_n(2);
    str_input.lookahead(1);

    assert_eq!(buffered.peek(), str_input.peek());

    buffered.skip_n(8);
    str_input.skip_n(8);
    assert_eq!(buffered.peek(), str_input.peek());
}

#[test]
fn buffered_skip_without_lookahead_matches_str_input() {
    let mut buffered = BufferedInput::new("ab".chars());
    buffered.skip();
    buffered.lookahead(1);

    let mut str_input = StrInput::new("ab");
    str_input.skip();
    str_input.lookahead(1);

    assert_eq!(buffered.peek(), str_input.peek());
}

#[test]
fn buffered_raw_reads_use_logical_stream_front() {
    let mut buffered = BufferedInput::new("ab".chars());
    buffered.lookahead(1);

    let mut str_input = StrInput::new("ab");
    str_input.lookahead(1);

    assert_eq!(buffered.raw_read_ch(), str_input.raw_read_ch());
    buffered.lookahead(1);
    str_input.lookahead(1);
    assert_eq!(buffered.peek(), str_input.peek());
}

#[test]
fn buffered_buflen_matches_str_input_lookahead_window() {
    let mut buffered = BufferedInput::new("ab".chars());
    buffered.lookahead(2);
    buffered.skip();
    buffered.skip();

    let mut str_input = StrInput::new("ab");
    str_input.lookahead(2);
    str_input.skip();
    str_input.skip();

    assert_eq!(buffered.buflen(), str_input.buflen());
    assert_eq!(buffered.buf_is_empty(), str_input.buf_is_empty());
    assert_eq!(buffered.peek(), str_input.peek());
}

#[test]
fn non_printable_source_characters_are_rejected_by_both_inputs() {
    let prefix = "x".repeat(80);
    for character in [
        '\0', '\u{1}', '\u{1f}', '\u{7f}', '\u{80}', '\u{84}', '\u{86}', '\u{9f}', '\u{fffe}',
        '\u{ffff}',
    ] {
        let inputs = [
            format!("key: {prefix}{character}after\n"),
            format!("'{prefix}{character}after'\n"),
            format!("\"{prefix}{character}after\"\n"),
            format!("key: |\n  {prefix}{character}after\n"),
            format!("key: >\n  {prefix}{character}after\n"),
            format!("# {prefix}{character}after\nkey: value\n"),
        ];

        for input in &inputs {
            assert_eq!(
                first_str_error(input).kind(),
                &ErrorKind::UnexpectedCharacter { character },
                "string input accepted {input:?}",
            );
            assert_eq!(
                first_iter_error(input).kind(),
                &ErrorKind::UnexpectedCharacter { character },
                "iterator input accepted {input:?}",
            );
        }
    }
}

#[test]
fn central_validation_defends_legacy_input_chunk_overrides() {
    let prefix = "x".repeat(80);
    for character in ['\u{1}', '\u{7f}', '\u{80}', '\u{9f}'] {
        let input = format!("key: {prefix}{character}after\n");
        let error = Parser::new(LegacyChunkInput::new(&input))
            .find_map(Result::err)
            .expect("input should fail");
        assert_eq!(
            error.kind(),
            &ErrorKind::UnexpectedCharacter { character },
            "legacy chunk override accepted {character:?}",
        );
    }
}

#[test]
fn under_indented_flow_scalar_spans_match_between_input_backends() {
    let input = "a:\n  [\nfoo]\n";
    let str_events = Parser::new_from_str(input)
        .collect::<Result<Vec<_>, _>>()
        .expect("string input should accept an under-indented flow entry");
    let iter_events = Parser::new_from_iter(input.chars())
        .collect::<Result<Vec<_>, _>>()
        .expect("iterator input should accept an under-indented flow entry");

    let mut scalar_spans = Vec::new();
    for events in [&str_events, &iter_events] {
        let (_, span) = events
            .iter()
            .find(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == "foo"))
            .expect("flow entry should be present");
        assert_eq!(span.start, Marker::new(7, 3, 0));
        assert_eq!(span.end, Marker::new(10, 3, 3));
        scalar_spans.push(span);
    }

    assert_eq!(scalar_spans[0].slice(input), Some("foo"));
    assert_eq!(scalar_spans[1].start.byte_offset(), None);
}

#[test]
fn escaped_nul_in_double_quoted_scalar_remains_valid() {
    let input = "\"\\0\"\n";

    assert_eq!(first_str_scalar(input), "\0");
    assert_eq!(first_iter_scalar(input), "\0");
}
