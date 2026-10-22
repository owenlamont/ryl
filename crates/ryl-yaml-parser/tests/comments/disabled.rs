use crate::support;
use std::{borrow::Cow, cell::Cell, rc::Rc};

use ryl_yaml_parser::{
    BorrowedInput, BufferedInput, ErrorKind, Event, FallibleBufferedInput, Marker,
    Options, Parser, ScalarStyle, ScanError, Scanner, Span, StrInput, StructureStyle,
    Tag, Token, TokenType,
};

fn no_comments() -> Options {
    ryl_yaml_parser::options! {
        emit_comments: false,
    }
}

fn no_comments_with_zero_buffer() -> Options {
    ryl_yaml_parser::options! {
        emit_comments: false,
        max_buffered_comment_events: 0,
    }
}

fn parse_str(source: &str, options: Options) -> Result<Vec<Event<'_>>, ScanError> {
    Parser::with_options(StrInput::new(source), options)
        .map(|result| result.map(|(event, _)| event))
        .collect()
}

fn parse_iter(
    source: &str,
    options: Options,
) -> Result<Vec<Event<'static>>, ScanError> {
    Parser::with_options(BufferedInput::new(source.chars()), options)
        .map(|result| result.map(|(event, _)| event))
        .collect()
}

fn scan_str(source: &str, options: Options) -> Result<Vec<Token<'_>>, ScanError> {
    Scanner::with_options(StrInput::new(source), options).collect()
}

fn scan_iter(source: &str, options: Options) -> Result<Vec<Token<'static>>, ScanError> {
    Scanner::with_options(BufferedInput::new(source.chars()), options).collect()
}

fn scan_non_comment_prefix_until_error<'input, T>(
    mut scanner: Scanner<'input, T>,
) -> (Vec<Token<'input>>, usize, ScanError)
where
    T: BorrowedInput<'input>,
{
    let mut prefix = Vec::new();
    let mut comment_count = 0;

    loop {
        match scanner.next() {
            Some(Ok(token)) => {
                if support::is_comment_token(token.token_type()) {
                    comment_count += 1;
                } else {
                    prefix.push(token);
                }
            }
            Some(Err(error)) => {
                assert!(scanner.next().is_none(), "scanner must fuse after an error");
                return (prefix, comment_count, error);
            }
            None => panic!("expected scanner error"),
        }
    }
}

fn parse_non_comment_prefix_until_error<'input, T>(
    mut parser: Parser<'input, T>,
) -> (Vec<(Event<'input>, Span)>, usize, ScanError)
where
    T: BorrowedInput<'input>,
{
    let mut prefix = Vec::new();
    let mut comment_count = 0;

    loop {
        match parser.next() {
            Some(Ok(event)) => {
                if support::is_comment_event(&(event.0)) {
                    comment_count += 1;
                } else {
                    prefix.push(event);
                }
            }
            Some(Err(error)) => {
                assert!(parser.next().is_none(), "parser must fuse after an error");
                return (prefix, comment_count, error);
            }
            None => panic!("expected parser error"),
        }
    }
}

fn assert_no_comment_events(events: &[Event<'_>]) {
    assert!(
        events.iter().all(|event| !support::is_comment_event(event)),
        "comment event escaped suppression: {events:?}",
    );
}

fn assert_no_comment_tokens(tokens: &[Token<'_>]) {
    assert!(
        tokens
            .iter()
            .all(|token| !support::is_comment_token(token.token_type())),
        "comment token escaped suppression: {tokens:?}",
    );
}

fn assert_parser_error(source: &str, expected: &ErrorKind) {
    let str_error = Parser::with_options(StrInput::new(source), no_comments())
        .find_map(Result::err)
        .expect("string parser should reject invalid YAML");
    assert_eq!(str_error.kind(), expected, "string input: {source:?}");

    let iter_error =
        Parser::with_options(BufferedInput::new(source.chars()), no_comments())
            .find_map(Result::err)
            .expect("iterator parser should reject invalid YAML");
    assert_eq!(iter_error.kind(), expected, "iterator input: {source:?}");
}

type ExpectedScalar = Option<(ScalarStyle, &'static str)>;

fn has_completed_token(tokens: &[Token<'_>], expected_scalar: &ExpectedScalar) -> bool {
    match expected_scalar {
        Some((style, value)) => tokens.iter().any(|token| {
            matches!(token.token_type(), TokenType::Scalar(actual_style, actual_value)
                if actual_style == style && actual_value == value)
        }),
        None => {
            tokens
                .iter()
                .any(|token| matches!(token.token_type(), TokenType::BlockMappingStart))
                && tokens
                    .iter()
                    .any(|token| matches!(token.token_type(), TokenType::Key))
        }
    }
}

fn has_completed_event(
    events: &[(Event<'_>, Span)],
    expected_scalar: &ExpectedScalar,
) -> bool {
    match expected_scalar {
        Some((style, value)) => events.iter().any(|(event, _)| {
            matches!(event, Event::Scalar(actual_value, actual_style, ..)
                if actual_style == style && actual_value == value)
        }),
        None => events
            .iter()
            .any(|(event, _)| matches!(event, Event::MappingStart(..))),
    }
}

fn assert_scanner_prefix<'input, T>(
    context: &str,
    enabled: Scanner<'input, T>,
    disabled: Scanner<'input, T>,
    expected_error: &ErrorKind,
    expected_scalar: &ExpectedScalar,
) where
    T: BorrowedInput<'input>,
{
    let (enabled_tokens, enabled_comments, enabled_error) =
        scan_non_comment_prefix_until_error(enabled);
    let (disabled_tokens, disabled_comments, disabled_error) =
        scan_non_comment_prefix_until_error(disabled);

    assert_eq!(enabled_error.kind(), expected_error, "{context}");
    assert_eq!(disabled_error.kind(), enabled_error.kind(), "{context}");
    assert_eq!(disabled_error.marker(), enabled_error.marker(), "{context}");
    assert_eq!(disabled_tokens, enabled_tokens, "{context}");
    assert!(
        enabled_comments > 0,
        "{context}: scanner comment emission must match the compiled feature"
    );
    assert_eq!(disabled_comments, 0, "{context}");
    assert!(
        has_completed_token(&enabled_tokens, expected_scalar),
        "{context}: completed token was missing"
    );
}

fn assert_parser_prefix<'input, T>(
    context: &str,
    enabled: Parser<'input, T>,
    disabled: Parser<'input, T>,
    expected_error: &ErrorKind,
    expected_scalar: &ExpectedScalar,
) where
    T: BorrowedInput<'input>,
{
    let (enabled_events, enabled_comments, enabled_error) =
        parse_non_comment_prefix_until_error(enabled);
    let (disabled_events, disabled_comments, disabled_error) =
        parse_non_comment_prefix_until_error(disabled);

    assert_eq!(enabled_error.kind(), expected_error, "{context}");
    assert_eq!(disabled_error.kind(), enabled_error.kind(), "{context}");
    assert_eq!(disabled_error.marker(), enabled_error.marker(), "{context}");
    assert_eq!(disabled_events, enabled_events, "{context}");
    assert!(
        enabled_comments > 0,
        "{context}: parser comment emission must match the compiled feature"
    );
    assert_eq!(disabled_comments, 0, "{context}");
    assert!(
        has_completed_event(&enabled_events, expected_scalar),
        "{context}: completed event was missing"
    );
}

fn comment_run(count: usize) -> String {
    let mut comments = String::new();
    for index in 0..count {
        comments.push_str("# comment ");
        comments.push_str(&index.to_string());
        comments.push('\n');
    }
    comments
}

#[test]
fn scanner_suppresses_every_lexical_comment_for_string_and_iterator_input() {
    let yaml = concat!(
        "# before document\n",
        "--- # after document start\n",
        "key: value # right\n",
        "flow: [one, # after flow entry\n",
        "  two] # after flow end\n",
        "... # after document end\n",
        "# before stream end\n",
    );

    let str_tokens =
        scan_str(yaml, no_comments()).expect("string scanner should accept comments");
    assert_no_comment_tokens(&str_tokens);

    let iter_tokens = scan_iter(yaml, no_comments())
        .expect("iterator scanner should accept comments");
    assert_no_comment_tokens(&iter_tokens);

    for tokens in [&str_tokens[..], &iter_tokens[..]] {
        assert!(tokens.iter().any(|token| {
            matches!(token.token_type(), TokenType::Scalar(_, value) if value == "value")
        }));
        assert!(tokens.iter().any(|token| {
            matches!(token.token_type(), TokenType::Scalar(_, value) if value == "two")
        }));
    }
}

#[test]
fn parser_suppresses_every_comment_event_for_string_and_iterator_input() {
    let yaml = concat!(
        "# leading\n",
        "root: # above sequence\n",
        "  - one # right\n",
        "  - [two, # flow\n",
        "     three]\n",
        "# trailing\n",
    );

    let str_events =
        parse_str(yaml, no_comments()).expect("string parser should accept comments");
    assert_no_comment_events(&str_events);

    let iter_events = parse_iter(yaml, no_comments())
        .expect("iterator parser should accept comments");
    assert_no_comment_events(&iter_events);
}

#[test]
fn disabled_comments_match_enabled_yaml_semantics_after_filtering_comments() {
    let yaml = concat!(
        "# header\n",
        "---\n",
        "root: # before sequence\n",
        "  - one # right\n",
        "  - [two, # in flow\n",
        "     three]\n",
        "empty: # empty value\n",
        "next: &anchor value\n",
        "alias: *anchor # alias\n",
        "# footer\n",
    );

    let mut enabled_str = parse_str(yaml, Options::default())
        .expect("enabled string parser should accept YAML");
    enabled_str.retain(|event| !support::is_comment_event(event));
    let disabled_str = parse_str(yaml, no_comments())
        .expect("disabled string parser should accept YAML");
    assert_eq!(disabled_str, enabled_str);

    let mut enabled_iter = parse_iter(yaml, Options::default())
        .expect("enabled iterator parser should accept YAML");
    enabled_iter.retain(|event| !support::is_comment_event(event));
    let disabled_iter = parse_iter(yaml, no_comments())
        .expect("disabled iterator parser should accept YAML");
    assert_eq!(disabled_iter, enabled_iter);
}

#[test]
fn comment_after_leading_document_end_does_not_start_implicit_document() {
    // Minimized from an options fuzz failure: `document_start` first consumes `...`, which can
    // expose a comment that was not visible to the parser's normal pre-dispatch comment check.
    let yaml = "...\n#";

    let mut enabled = parse_str(yaml, Options::default())
        .expect("leading document end and comment should not open a document");
    {
        assert!(matches!(
            enabled.as_slice(),
            [Event::StreamStart, Event::Comment(..), Event::StreamEnd]
        ));
    }
    enabled.retain(|event| !support::is_comment_event(event));

    let disabled = parse_str(yaml, no_comments())
        .expect("comment suppression should preserve the empty stream");

    assert_eq!(disabled, enabled);
}

#[test]
fn disabled_comments_do_not_skip_document_end_after_directive_comment() {
    fn assert_error<'input, T>(
        context: &str,
        enabled: Parser<'input, T>,
        disabled: Parser<'input, T>,
    ) where
        T: BorrowedInput<'input>,
    {
        let (enabled_events, enabled_comments, enabled_error) =
            parse_non_comment_prefix_until_error(enabled);
        let (disabled_events, disabled_comments, disabled_error) =
            parse_non_comment_prefix_until_error(disabled);

        assert_eq!(enabled_comments, usize::from(true), "{context}");
        assert_eq!(disabled_comments, 0, "{context}");
        assert_eq!(disabled_events, enabled_events, "{context}");
        assert!(
            matches!(enabled_events.as_slice(), [(Event::StreamStart, _)]),
            "unexpected parser prefix: {context}"
        );

        assert_eq!(
            enabled_error.kind(),
            &ErrorKind::ExpectedDocumentStart,
            "{context}"
        );
        assert_eq!(disabled_error.kind(), enabled_error.kind(), "{context}");
        assert_eq!(
            (
                enabled_error.marker().index(),
                enabled_error.marker().line(),
                enabled_error.marker().col(),
            ),
            (5, 3, 0),
            "error should point at the document-end marker: {context}"
        );
        assert_eq!(disabled_error.marker(), enabled_error.marker(), "{context}");
    }

    // Minimized from options fuzz artifact 2bcfc3e81fd77f0f8066d92f74a7615f2a248234.
    let yaml = "%a\n#\n...";

    assert_error(
        "string parser",
        Parser::with_options(StrInput::new(yaml), Options::default()),
        Parser::with_options(StrInput::new(yaml), no_comments()),
    );
    assert_error(
        "buffered parser",
        Parser::with_options(BufferedInput::new(yaml.chars()), Options::default()),
        Parser::with_options(BufferedInput::new(yaml.chars()), no_comments()),
    );
}

#[test]
fn disabled_comments_preserve_non_comment_event_spans() {
    // The first fixture is minimized from the options fuzz failure. The second covers the other
    // synthetic-span consumer: an empty document whose content is only a comment.
    for yaml in [".\n#", "---\n# comment\n"] {
        let mut enabled = Parser::with_options(StrInput::new(yaml), Options::default())
            .collect::<Result<Vec<_>, _>>()
            .expect("enabled string parser should accept YAML");
        assert!(
            enabled
                .iter()
                .any(|(event, _)| support::is_comment_event(event)),
            "parser comment emission must match the compiled feature: {yaml:?}"
        );
        enabled.retain(|(event, _)| !support::is_comment_event(event));

        let disabled = Parser::with_options(StrInput::new(yaml), no_comments())
            .collect::<Result<Vec<_>, _>>()
            .expect("disabled string parser should accept YAML");

        assert_eq!(disabled, enabled, "{yaml:?}");
    }
}

fn empty_node_comment_cases() -> [(&'static str, Vec<Event<'static>>); 5] {
    // YAML 1.2.2 §§6.6, 6.9 and 7.2 permit EOF-terminated comments and empty nodes
    // whose anchor/tag properties remain significant, including an empty explicit key.
    let null = |anchor| Event::Scalar("~".into(), ScalarStyle::Plain, anchor, None);
    let local_empty = Event::Scalar(
        "".into(),
        ScalarStyle::Plain,
        1,
        Some(Cow::Owned(Tag::new("!", "local"))),
    );
    [
        (
            "? # café",
            vec![
                Event::MappingStart(StructureStyle::Block, 0, None),
                null(0),
                null(0),
                Event::MappingEnd,
            ],
        ),
        (
            "? &a # café",
            vec![
                Event::MappingStart(StructureStyle::Block, 0, None),
                null(1),
                null(0),
                Event::MappingEnd,
            ],
        ),
        (
            "!!str # café",
            vec![Event::Scalar(
                "".into(),
                ScalarStyle::Plain,
                0,
                Some(Cow::Owned(Tag::with_original_handle(
                    "tag:yaml.org,2002:",
                    "str",
                    "!!",
                ))),
            )],
        ),
        ("!local &a # café", vec![local_empty.clone()]),
        ("&a # first\n!local # café", vec![local_empty]),
    ]
}

#[test]
fn comments_at_eof_preserve_empty_nodes_and_properties_across_backends() {
    for (template, body) in empty_node_comment_cases() {
        let mut expected_events =
            vec![Event::StreamStart, Event::DocumentStart(false, None)];
        expected_events.extend(body);
        expected_events.extend([Event::DocumentEnd, Event::StreamEnd]);
        for line_break in ["\n", "\r", "\r\n"] {
            let yaml =
                format!("# pré{line_break}{}", template.replace('\n', line_break));
            let eof = Marker::new(
                yaml.chars().count(),
                template.matches('\n').count() + 2,
                template.rsplit('\n').next().unwrap().chars().count(),
            );
            let mut reference = None;
            for strict_indentation in [false, true] {
                for emit_comments in [false, true] {
                    let options = ryl_yaml_parser::options! {
                        strict_indentation: strict_indentation,
                        emit_comments: emit_comments,
                    };
                    let traces = non_comment_traces_to_eof(&yaml, &options, eof);
                    assert_eq!(
                        traces
                            .0
                            .iter()
                            .map(|(event, _)| event.clone())
                            .collect::<Vec<_>>(),
                        expected_events,
                        "{yaml:?}, {options:?}"
                    );
                    let reference = reference.get_or_insert_with(|| traces.clone());
                    assert_eq!(&traces, reference, "{yaml:?}, {options:?}");
                }
            }
        }
    }
}

/// Every backend's non-comment events and tokens for a stream ending at `eof`, after checking
/// that the backends agree and emit one comment per `#` exactly when comments are enabled.
fn non_comment_traces_to_eof<'y>(
    yaml: &'y str,
    options: &Options,
    eof: Marker,
) -> (Vec<(Event<'y>, Span)>, Vec<Token<'y>>) {
    let context = format!("{yaml:?}, {options:?}");
    let expected_comments = if options.emit_comments {
        yaml.matches('#').count()
    } else {
        0
    };
    let events = [
        Parser::new_from_str_with_options(yaml, options.clone())
            .collect::<Result<Vec<_>, _>>(),
        Parser::new_from_iter_with_options(yaml.chars(), options.clone()).collect(),
        Parser::new_from_fallible_iter_with_options(
            yaml.chars().map(Ok),
            options.clone(),
        )
        .collect(),
    ]
    .map(|events| {
        let mut events = events.unwrap_or_else(|error| panic!("{context}: {error}"));
        let total = events.len();
        events.retain(|(event, _)| !support::is_comment_event(event));
        assert_eq!(total - events.len(), expected_comments, "{context}");
        assert_eq!(events.last().unwrap().1, Span::empty(eof), "{context}");
        events
    });
    let tokens = [
        Scanner::with_options(StrInput::new(yaml), options.clone())
            .collect::<Result<Vec<_>, _>>(),
        Scanner::with_options(BufferedInput::new(yaml.chars()), options.clone())
            .collect(),
        Scanner::with_options(
            FallibleBufferedInput::new(yaml.chars().map(Ok)),
            options.clone(),
        )
        .collect(),
    ]
    .map(|tokens| {
        let mut tokens = tokens.unwrap_or_else(|error| panic!("{context}: {error}"));
        let total = tokens.len();
        tokens.retain(|token| !support::is_comment_token(token.token_type()));
        assert_eq!(total - tokens.len(), expected_comments, "{context}");
        let stream_end = tokens.last().unwrap();
        assert_eq!(stream_end.token_type(), &TokenType::StreamEnd, "{context}");
        assert_eq!(stream_end.span(), Span::empty(eof), "{context}");
        tokens
    });
    let [events, iter_events, fallible_events] = events;
    assert_eq!(iter_events, events, "{context}");
    assert_eq!(fallible_events, events, "{context}");
    let [tokens, iter_tokens, fallible_tokens] = tokens;
    assert_eq!(iter_tokens, tokens, "{context}");
    assert_eq!(fallible_tokens, tokens, "{context}");
    (events, tokens)
}

#[test]
fn invalid_controls_in_eof_comments_keep_exact_error_positions() {
    // YAML 1.2.2 §5.1 excludes these controls from printable comment content.
    for line_break in ["\n", "\r", "\r\n"] {
        for prefix in ["", "? ", "&a !local ", "| ", "%YAML 1.2 "] {
            for character in ['\0', '\u{1}', '\u{7f}', '\u{80}', '\u{9f}'] {
                let yaml = format!("# pré{line_break}{prefix}# café {character}");
                let marker = Marker::new(
                    yaml.chars().count() - 1,
                    2,
                    prefix.chars().count() + 7,
                );
                for emit_comments in [false, true] {
                    assert_invalid_eof_comment_control(
                        &yaml,
                        character,
                        marker,
                        emit_comments,
                    );
                }
            }
        }
    }
}

fn assert_invalid_eof_comment_control(
    yaml: &str,
    character: char,
    marker: Marker,
    emit_comments: bool,
) {
    let options = ryl_yaml_parser::options! { emit_comments: emit_comments };
    let context = format!("{yaml:?}, comments={emit_comments}");
    let parser_traces = [
        parse_non_comment_prefix_until_error(Parser::new_from_str_with_options(
            yaml,
            options.clone(),
        )),
        parse_non_comment_prefix_until_error(Parser::new_from_iter_with_options(
            yaml.chars(),
            options.clone(),
        )),
        parse_non_comment_prefix_until_error(
            Parser::new_from_fallible_iter_with_options(
                yaml.chars().map(Ok),
                options.clone(),
            ),
        ),
    ];
    let scanner_traces = [
        scan_non_comment_prefix_until_error(Scanner::with_options(
            StrInput::new(yaml),
            options.clone(),
        )),
        scan_non_comment_prefix_until_error(Scanner::with_options(
            BufferedInput::new(yaml.chars()),
            options.clone(),
        )),
        scan_non_comment_prefix_until_error(Scanner::with_options(
            FallibleBufferedInput::new(yaml.chars().map(Ok)),
            options,
        )),
    ];
    for error in parser_traces
        .iter()
        .map(|trace| &trace.2)
        .chain(scanner_traces.iter().map(|trace| &trace.2))
    {
        assert_eq!(
            error.kind(),
            &ErrorKind::UnexpectedCharacter { character },
            "{context}"
        );
        assert_eq!(*error.marker(), marker, "{context}");
    }
    for trace in &parser_traces[1..] {
        assert_eq!(trace, &parser_traces[0], "{context}");
    }
    for trace in &scanner_traces[1..] {
        assert_eq!(trace, &scanner_traces[0], "{context}");
    }
    assert_eq!(
        parser_traces[0].2.marker().byte_offset(),
        Some(yaml.len() - character.len_utf8()),
        "{context}"
    );
    assert_eq!(
        scanner_traces[0].2.marker().byte_offset(),
        Some(yaml.len() - character.len_utf8()),
        "{context}"
    );
}

#[test]
fn disabled_comments_preserve_events_and_errors_for_invalid_yaml() {
    // The own-line case preserves the older options fuzz failure. The right-comment case reaches
    // the separate batched-comment error path, and the node-property case protects its interaction
    // with comment-separated anchors and tags.
    for (name, yaml) in [
        ("own-line comment", " :\n#\n  a\u{1}\nx"),
        ("right comment", " : #\n  a\u{1}\nx"),
        ("pending node properties", "&a !t\n# comment\na\u{1}\n"),
        ("after leading document end", "...\n# comment\na\u{1}\n"),
    ] {
        let (enabled_events, enabled_comments, enabled_error) =
            parse_non_comment_prefix_until_error(Parser::with_options(
                StrInput::new(yaml),
                Options::default(),
            ));
        let (disabled_events, disabled_comments, disabled_error) =
            parse_non_comment_prefix_until_error(Parser::with_options(
                StrInput::new(yaml),
                no_comments(),
            ));

        assert!(
            enabled_comments > 0,
            "parser comment emission must match the compiled feature: {name}"
        );
        assert_eq!(disabled_comments, 0, "{name}");
        assert_eq!(disabled_events, enabled_events, "{name}");
        assert_eq!(disabled_error.kind(), enabled_error.kind(), "{name}");
        assert_eq!(disabled_error.marker(), enabled_error.marker(), "{name}");

        let expected_error = ErrorKind::UnexpectedCharacter { character: '\u{1}' };
        assert_eq!(enabled_error.kind(), &expected_error, "{name}");
        assert!(
            enabled_events.iter().all(|(event, _)| !matches!(
                event,
                Event::MappingEnd | Event::DocumentEnd
            )),
            "parser emitted closing events after the error: {name}"
        );
    }
}

#[test]
fn disabled_comments_bypass_zero_buffer_limit_in_every_ambiguous_entry() {
    let comments = comment_run(64);
    let cases = [
        (
            "block mapping value",
            format!("key: {comments}next: value\n"),
        ),
        ("block sequence entry", format!("- {comments}- value\n")),
        (
            "indentless sequence entry",
            format!("key:\n- {comments}next: value\n"),
        ),
        (
            "later indentless sequence entry",
            format!("key:\n- first\n- {comments}  second\nnext: value\n"),
        ),
        ("flow mapping value", format!("root: {{key: {comments}}}\n")),
    ];

    for (name, yaml) in cases {
        let str_events = parse_str(&yaml, no_comments_with_zero_buffer())
            .unwrap_or_else(|error| panic!("{name}, string input: {error}"));
        assert_no_comment_events(&str_events);

        let iter_events = parse_iter(&yaml, no_comments_with_zero_buffer())
            .unwrap_or_else(|error| panic!("{name}, iterator input: {error}"));
        assert_no_comment_events(&iter_events);
    }
}

#[test]
fn suppressing_comments_does_not_relax_comment_syntax_validation() {
    let cases = [
        (
            "key: \"value\"# unseparated\n",
            ErrorKind::CommentNotSeparated,
        ),
        ("block: ># unseparated\n", ErrorKind::CommentNotSeparated),
        (
            "word1  # interrupts scalar\nword2\n",
            ErrorKind::CommentInterceptedScalar,
        ),
        (
            "? # first\n  # second\n\tkey\n: value\n",
            ErrorKind::TabNotAllowed,
        ),
        (
            "key: \"value\" # control \u{1}\n",
            ErrorKind::UnexpectedCharacter { character: '\u{1}' },
        ),
    ];

    for (source, expected) in cases {
        assert_parser_error(source, &expected);
    }
}

#[test]
fn disabled_comments_preserve_non_comment_prefixes_before_errors() {
    for (name, source, expected_error, expected_scalar) in [
        (
            "completed explicit key",
            "? # ignored\n\tbad\n",
            ErrorKind::TabNotAllowed,
            None,
        ),
        (
            "completed quoted scalar",
            "\"value\"\n# ignored\n@\n",
            ErrorKind::UnexpectedCharacter { character: '@' },
            Some((ScalarStyle::DoubleQuoted, "value")),
        ),
        (
            "completed plain scalar",
            "foo\n# ignored\n@\n",
            ErrorKind::UnexpectedCharacter { character: '@' },
            Some((ScalarStyle::Plain, "foo")),
        ),
    ] {
        assert_scanner_prefix(
            &format!("{name}, string scanner"),
            Scanner::with_options(StrInput::new(source), Options::default()),
            Scanner::with_options(StrInput::new(source), no_comments()),
            &expected_error,
            &expected_scalar,
        );
        assert_scanner_prefix(
            &format!("{name}, iterator scanner"),
            Scanner::with_options(
                BufferedInput::new(source.chars()),
                Options::default(),
            ),
            Scanner::with_options(BufferedInput::new(source.chars()), no_comments()),
            &expected_error,
            &expected_scalar,
        );
        assert_parser_prefix(
            &format!("{name}, string parser"),
            Parser::with_options(StrInput::new(source), Options::default()),
            Parser::with_options(StrInput::new(source), no_comments()),
            &expected_error,
            &expected_scalar,
        );
        assert_parser_prefix(
            &format!("{name}, iterator parser"),
            Parser::with_options(
                BufferedInput::new(source.chars()),
                Options::default(),
            ),
            Parser::with_options(BufferedInput::new(source.chars()), no_comments()),
            &expected_error,
            &expected_scalar,
        );
    }
}

#[test]
fn ignored_tab_prefixed_comments_preserve_completed_scalars_before_later_errors() {
    let source = "root:\n  key: \"value\"\n\t# ignored\n\t# invalid \u{1}\n";
    let expected_error = ErrorKind::UnexpectedCharacter { character: '\u{1}' };
    let expected_scalar = Some((ScalarStyle::DoubleQuoted, "value"));

    for strict_indentation in [false, true] {
        let enabled =
            ryl_yaml_parser::options! { strict_indentation: strict_indentation };
        let disabled = ryl_yaml_parser::options! {
            strict_indentation: strict_indentation,
            emit_comments: false,
        };

        assert_scanner_prefix(
            &format!("strict={strict_indentation}, string scanner"),
            Scanner::with_options(StrInput::new(source), enabled.clone()),
            Scanner::with_options(StrInput::new(source), disabled.clone()),
            &expected_error,
            &expected_scalar,
        );
        assert_scanner_prefix(
            &format!("strict={strict_indentation}, iterator scanner"),
            Scanner::with_options(BufferedInput::new(source.chars()), enabled.clone()),
            Scanner::with_options(BufferedInput::new(source.chars()), disabled.clone()),
            &expected_error,
            &expected_scalar,
        );
        assert_parser_prefix(
            &format!("strict={strict_indentation}, string parser"),
            Parser::with_options(StrInput::new(source), enabled.clone()),
            Parser::with_options(StrInput::new(source), disabled.clone()),
            &expected_error,
            &expected_scalar,
        );
        assert_parser_prefix(
            &format!("strict={strict_indentation}, iterator parser"),
            Parser::with_options(BufferedInput::new(source.chars()), enabled),
            Parser::with_options(BufferedInput::new(source.chars()), disabled),
            &expected_error,
            &expected_scalar,
        );
    }
}

#[test]
fn disabled_comments_preserve_required_block_scalar_error_prefix() {
    fn assert_scanner<'input, T>(
        context: &str,
        enabled: Scanner<'input, T>,
        disabled: Scanner<'input, T>,
    ) where
        T: BorrowedInput<'input>,
    {
        let (enabled_tokens, _, enabled_error) =
            scan_non_comment_prefix_until_error(enabled);
        let (disabled_tokens, disabled_comments, disabled_error) =
            scan_non_comment_prefix_until_error(disabled);

        assert_eq!(
            enabled_error.kind(),
            &ErrorKind::SimpleKeyExpected,
            "{context}"
        );
        assert_eq!(enabled_error.marker().index(), 2, "{context}");
        assert_eq!(disabled_error.kind(), enabled_error.kind(), "{context}");
        assert_eq!(disabled_error.marker(), enabled_error.marker(), "{context}");
        assert_eq!(disabled_comments, 0, "{context}");
        assert_eq!(disabled_tokens, enabled_tokens, "{context}");
        assert!(
            enabled_tokens.iter().all(|token| !matches!(
                token.token_type(),
                TokenType::Scalar(ScalarStyle::Literal, _)
            )),
            "block scalar escaped before its required-key error: {context}"
        );
    }

    fn assert_parser<'input, T>(
        context: &str,
        enabled: Parser<'input, T>,
        disabled: Parser<'input, T>,
    ) where
        T: BorrowedInput<'input>,
    {
        let (enabled_events, _, enabled_error) =
            parse_non_comment_prefix_until_error(enabled);
        let (disabled_events, disabled_comments, disabled_error) =
            parse_non_comment_prefix_until_error(disabled);

        assert_eq!(
            enabled_error.kind(),
            &ErrorKind::SimpleKeyExpected,
            "{context}"
        );
        assert_eq!(enabled_error.marker().index(), 2, "{context}");
        assert_eq!(disabled_error.kind(), enabled_error.kind(), "{context}");
        assert_eq!(disabled_error.marker(), enabled_error.marker(), "{context}");
        assert_eq!(disabled_comments, 0, "{context}");
        assert_eq!(disabled_events, enabled_events, "{context}");
        assert!(
            enabled_events.iter().all(|(event, _)| !matches!(
                event,
                Event::Scalar(_, ScalarStyle::Literal, ..)
            )),
            "block scalar escaped before its required-key error: {context}"
        );
    }

    // Minimized from options fuzz artifact 5f02d987814c1292d7dd4cfa376cded917d93229.
    let yaml = "-\n| #";

    assert_scanner(
        "string scanner",
        Scanner::with_options(StrInput::new(yaml), Options::default()),
        Scanner::with_options(StrInput::new(yaml), no_comments()),
    );
    assert_scanner(
        "buffered scanner",
        Scanner::with_options(BufferedInput::new(yaml.chars()), Options::default()),
        Scanner::with_options(BufferedInput::new(yaml.chars()), no_comments()),
    );
    assert_parser(
        "string parser",
        Parser::with_options(StrInput::new(yaml), Options::default()),
        Parser::with_options(StrInput::new(yaml), no_comments()),
    );
    assert_parser(
        "buffered parser",
        Parser::with_options(BufferedInput::new(yaml.chars()), Options::default()),
        Parser::with_options(BufferedInput::new(yaml.chars()), no_comments()),
    );
}

#[test]
fn streaming_comment_failures_preserve_source_errors_in_all_emission_modes() {
    fn failing_input(
        source: &str,
    ) -> impl Iterator<Item = Result<char, ErrorKind>> + '_ {
        source
            .chars()
            .map(Ok)
            .chain(core::iter::once(Err(ErrorKind::InputByteLimitExceeded {
                limit: source.len(),
            })))
            .chain(core::iter::from_fn(|| {
                panic!("comment scanning must not poll the source after its failure")
            }))
    }

    for source in [
        "# unfinished\n",
        "# unfinished café",
        "value # unfinished café",
        "%YAML 1.2 # unfinished café",
        "%TAG !e! tag:example.com,2026: # unfinished café",
        "%CUSTOM param # unfinished café",
        "block: | # unfinished café",
        "&anchor # unfinished café",
        "value\n... # unfinished café",
    ] {
        // `true` also exercises the always-available option when comment support is absent.
        for emit_comments in [false, true] {
            let options = ryl_yaml_parser::options! { emit_comments: emit_comments };
            let context = format!("{source:?}, emit_comments={emit_comments}");
            let (events, event_comments, parser_error) =
                parse_non_comment_prefix_until_error(
                    Parser::new_from_fallible_iter_with_options(
                        failing_input(source),
                        options.clone(),
                    ),
                );
            let (tokens, token_comments, scanner_error) =
                scan_non_comment_prefix_until_error(Scanner::with_options(
                    FallibleBufferedInput::new(failing_input(source)),
                    options,
                ));

            assert_eq!(
                parser_error.kind(),
                &ErrorKind::InputByteLimitExceeded {
                    limit: source.len()
                },
                "{context}",
            );
            assert_eq!(scanner_error, parser_error, "{context}");
            assert_eq!(
                parser_error.marker().index(),
                source.chars().count(),
                "{context}"
            );
            assert_eq!(
                parser_error.marker().line(),
                source.matches('\n').count() + 1,
                "{context}",
            );
            assert_eq!(
                parser_error.marker().col(),
                source.rsplit('\n').next().unwrap().chars().count(),
                "{context}",
            );
            assert!(
                events
                    .iter()
                    .all(|(event, _)| !matches!(event, Event::StreamEnd)),
                "{context}"
            );
            assert!(
                tokens
                    .iter()
                    .all(|token| !matches!(token.token_type(), TokenType::StreamEnd)),
                "{context}"
            );
            if !emit_comments {
                assert_eq!((event_comments, token_comments), (0, 0), "{context}");
            }
        }
    }
}

fn assert_literal_hash_scalars(events: &[Event<'_>]) {
    let expected = [
        (ScalarStyle::Plain, "value#plain"),
        (ScalarStyle::SingleQuoted, "# single"),
        (ScalarStyle::DoubleQuoted, "# double"),
        (ScalarStyle::Literal, "# block\n"),
    ];

    for (style, expected_value) in expected {
        assert!(
            events.iter().any(|event| {
                matches!(event, Event::Scalar(value, actual_style, ..)
                    if *actual_style == style && value == expected_value)
            }),
            "missing {style:?} scalar {expected_value:?} in {events:?}",
        );
    }
}

#[test]
fn hash_characters_inside_scalars_remain_data_when_comments_are_disabled() {
    let yaml = concat!(
        "plain: value#plain\n",
        "single: '# single'\n",
        "double: \"# double\"\n",
        "block: |\n",
        "  # block\n",
    );

    let str_events =
        parse_str(yaml, no_comments()).expect("string parser should accept scalars");
    assert_no_comment_events(&str_events);
    assert_literal_hash_scalars(&str_events);

    let iter_events =
        parse_iter(yaml, no_comments()).expect("iterator parser should accept scalars");
    assert_no_comment_events(&iter_events);
    assert_literal_hash_scalars(&iter_events);
}

#[test]
fn completed_plain_scalar_is_returned_before_long_suppressed_comment_tail_is_read() {
    let yaml = format!("foo\n{}...\n", comment_run(4_096));
    let total_chars = yaml.chars().count();
    let chars_read = Rc::new(Cell::new(0));
    let observed_chars_read = Rc::clone(&chars_read);
    let input = yaml.chars().inspect(move |_| {
        observed_chars_read.set(observed_chars_read.get() + 1);
    });
    let mut parser = Parser::with_options(BufferedInput::new(input), no_comments());

    loop {
        let (event, _) = parser
            .next_event()
            .expect("parser should return the completed scalar")
            .expect("the comment tail should not fail");
        match event {
            Event::Comment(..) => panic!("disabled comments must not be emitted"),
            Event::Scalar(value, ScalarStyle::Plain, ..) if value == "foo" => break,
            _ => {}
        }
    }

    assert!(
        chars_read.get() < 128,
        "parser read {} of {total_chars} characters before returning the completed scalar",
        chars_read.get(),
    );
}

#[test]
fn completed_plain_scalar_is_returned_before_fallible_suppressed_comment_tail_fails() {
    let yaml = format!("foo\n{}", comment_run(4_096));
    let source_error = ErrorKind::InputByteLimitExceeded { limit: 64 };
    let input = yaml
        .chars()
        .map(Ok)
        .chain(core::iter::once(Err(source_error.clone())));
    let mut parser =
        Parser::with_options(FallibleBufferedInput::new(input), no_comments());

    loop {
        match parser
            .next_event()
            .expect("parser should return the completed scalar")
        {
            Ok((Event::Comment(..), _)) => {
                panic!("disabled comments must not be emitted")
            }
            Ok((Event::Scalar(value, ScalarStyle::Plain, ..), _)) if value == "foo" => {
                break;
            }
            Ok(_) => {}
            Err(error) => {
                panic!("source failure overtook the completed scalar: {error}")
            }
        }
    }

    let error = parser
        .find_map(Result::err)
        .expect("source failure after the comment tail must remain visible");
    assert_eq!(error.kind(), &source_error);
}
