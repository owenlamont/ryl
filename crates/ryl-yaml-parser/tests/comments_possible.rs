use granit_parser::{
    input::{SkipTabs, WhitespaceResult},
    BorrowedInput, ErrorKind, Event, Input, Parser, ScanError, Scanner, Span, StrInput, Token,
};

struct CommentEnabledStrInput<'input> {
    inner: StrInput<'input>,
}

impl<'input> CommentEnabledStrInput<'input> {
    #[must_use]
    fn new(source: &'input str) -> Self {
        Self {
            inner: StrInput::new(source),
        }
    }
}

impl Input for CommentEnabledStrInput<'_> {
    fn lookahead(&mut self, count: usize) {
        self.inner.lookahead(count);
    }

    fn buflen(&self) -> usize {
        self.inner.buflen()
    }

    fn bufmaxlen(&self) -> usize {
        self.inner.bufmaxlen()
    }

    fn raw_read_ch(&mut self) -> char {
        self.inner.raw_read_ch()
    }

    fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
        self.inner.raw_read_non_breakz_ch()
    }

    fn skip(&mut self) {
        self.inner.skip();
    }

    fn skip_n(&mut self, count: usize) {
        self.inner.skip_n(count);
    }

    fn peek(&self) -> char {
        self.inner.peek()
    }

    fn peek_nth(&self, n: usize) -> char {
        self.inner.peek_nth(n)
    }

    fn byte_offset(&self) -> Option<usize> {
        self.inner.byte_offset()
    }

    fn slice_bytes(&self, start: usize, end: usize) -> Option<&str> {
        self.inner.slice_bytes(start, end)
    }

    fn may_contain_comments(&self) -> bool {
        true
    }

    fn skip_ws_to_eol(
        &mut self,
        skip_tabs: SkipTabs,
    ) -> (usize, Result<WhitespaceResult, ErrorKind>) {
        self.inner.skip_ws_to_eol(skip_tabs)
    }

    fn skip_ws_to_eol_blanks(&mut self, skip_tabs: SkipTabs) -> (usize, WhitespaceResult) {
        self.inner.skip_ws_to_eol_blanks(skip_tabs)
    }
}

impl<'input> BorrowedInput<'input> for CommentEnabledStrInput<'input> {
    fn slice_borrowed(&self, start: usize, end: usize) -> Option<&'input str> {
        self.inner.slice_borrowed(start, end)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ParseTrace<'input> {
    events: Vec<(Event<'input>, Span)>,
    error: Option<ScanError>,
}

fn scanner_tokens_fast_path(source: &str) -> Vec<Token<'_>> {
    Scanner::new(StrInput::new(source))
        .collect::<Result<_, _>>()
        .expect("valid YAML should scan without errors")
}

fn scanner_tokens_comment_enabled(source: &str) -> Vec<Token<'_>> {
    Scanner::new(CommentEnabledStrInput::new(source))
        .collect::<Result<_, _>>()
        .expect("valid YAML should scan without errors")
}

fn parser_trace_fast_path(source: &str) -> ParseTrace<'_> {
    let mut parser = Parser::new_from_str(source);
    parser_trace(&mut parser)
}

fn parser_trace_comment_enabled(source: &str) -> ParseTrace<'_> {
    let mut parser = Parser::new(CommentEnabledStrInput::new(source));
    parser_trace(&mut parser)
}

fn parser_trace<'input, T>(parser: &mut Parser<'input, T>) -> ParseTrace<'input>
where
    T: BorrowedInput<'input>,
{
    let mut events = Vec::new();
    let mut error = None;

    while let Some(next) = parser.next_event() {
        match next {
            Ok(event) => events.push(event),
            Err(err) => {
                error = Some(err);
                break;
            }
        }
    }

    ParseTrace { events, error }
}

fn assert_no_hash(source: &str, name: &str) {
    assert!(
        !source.contains('#'),
        "{name} fixture must exercise the no-comment fast path"
    );
    assert!(
        !StrInput::new(source).may_contain_comments(),
        "{name} should make StrInput disable comment probing"
    );
    assert!(
        CommentEnabledStrInput::new(source).may_contain_comments(),
        "{name} control input should keep comment probing enabled"
    );
}

#[test]
fn no_comment_fast_path_matches_comment_enabled_tokens_and_events() {
    for (name, source) in [
        ("block mapping", "a: b\n"),
        ("tab after value indicator", "a:\tb\n"),
        ("block sequence value", "a:\n  - b\n"),
        ("flow sequence", "[a, b]\n"),
        ("flow mapping", "{a: b}\n"),
        ("explicit document", "---\na: b\n...\n"),
    ] {
        assert_no_hash(source, name);

        assert_eq!(
            scanner_tokens_fast_path(source),
            scanner_tokens_comment_enabled(source),
            "{name} scanner token output differed"
        );
        assert_eq!(
            parser_trace_fast_path(source),
            parser_trace_comment_enabled(source),
            "{name} parser event output differed"
        );
    }
}

#[test]
fn invalid_no_comment_fast_path_matches_comment_enabled_event_prefixes() {
    for (name, source) in [
        ("unclosed flow sequence", "a: [1, 2\n"),
        ("extra flow sequence end", "key: [1, 2]]\n"),
        ("tab in value indentation", "a:\n\tb\n"),
        (
            "directive after implicit document",
            "a: b\n%YAML 1.2\n---\nc: d\n",
        ),
        ("invalid first indentless sequence entry", "a:\n-\nb"),
        ("reserved indicator", "@\n"),
    ] {
        assert_no_hash(source, name);

        let fast_path = parser_trace_fast_path(source);
        let comment_enabled = parser_trace_comment_enabled(source);

        assert!(fast_path.error.is_some(), "{name} should fail");
        assert_eq!(
            fast_path, comment_enabled,
            "{name} parser event prefix or error differed"
        );
    }
}
