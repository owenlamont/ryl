use std::borrow::Cow;

use granit_parser::{ErrorKind, Event, InputIoError, Marker, Parser, ScanError, Span};

/// Compare values, styles, and character-based spans across all built-in input backends.
/// Marker equality deliberately ignores the optional byte offsets of string input.
fn parse_all_inputs(source: &str) -> Vec<(Event<'_>, Span)> {
    let from_str = Parser::new_from_str(source)
        .collect::<Result<Vec<_>, _>>()
        .expect("string input should parse");
    let from_iter = Parser::new_from_iter(source.chars())
        .collect::<Result<Vec<_>, _>>()
        .expect("iterator input should parse");
    let from_fallible = Parser::new_from_fallible_iter(source.chars().map(Ok::<_, ErrorKind>))
        .collect::<Result<Vec<_>, _>>()
        .expect("fallible input should parse");
    assert_eq!(from_str, from_iter);
    assert_eq!(from_str, from_fallible);
    from_str
}

fn scalar<'events, 'input>(
    events: &'events [(Event<'input>, Span)],
) -> (&'events Cow<'input, str>, Span) {
    events
        .iter()
        .find_map(|(event, span)| match event {
            Event::Scalar(value, ..) => Some((value, *span)),
            _ => None,
        })
        .expect("expected a scalar")
}

fn terminal_error<'input>(
    mut parser: impl Iterator<Item = Result<(Event<'input>, Span), ScanError>>,
) -> ScanError {
    let error = parser
        .by_ref()
        .find_map(Result::err)
        .expect("expected an error");
    assert!(
        parser.next().is_none(),
        "an error must terminate the parser"
    );
    error
}

fn error_all_inputs(source: &str) -> ScanError {
    let from_str = terminal_error(Parser::new_from_str(source));
    let from_iter = terminal_error(Parser::new_from_iter(source.chars()));
    let from_fallible = terminal_error(Parser::new_from_fallible_iter(
        source.chars().map(Ok::<_, ErrorKind>),
    ));
    for other in [&from_iter, &from_fallible] {
        assert_eq!(from_str.kind(), other.kind());
        assert_eq!(from_str.marker(), other.marker());
    }
    from_str
}

#[test]
fn unescaped_ascii_runs_remain_borrowed_at_short_and_long_boundaries() {
    for quote in ['\'', '"'] {
        for length in [0, 1, 2, 3, 4, 7, 8, 15, 16, 31, 32, 127, 128, 4096] {
            let contents = "a".repeat(length);
            let source = format!("[{quote}{contents}{quote}, tail]\n");
            let events = parse_all_inputs(&source);
            let (value, span) = scalar(&events);
            assert_eq!(value, &contents);
            assert!(matches!(value, Cow::Borrowed(_)));
            assert_eq!(span.start, Marker::new(1, 1, 1));
            assert_eq!(span.end, Marker::new(length + 3, 1, length + 3));
            assert_eq!(span.byte_range(), Some(1..length + 3));
            assert_eq!(span.slice(&source), Some(&source[1..length + 3]));
        }
    }
}

#[test]
fn printable_ascii_and_unicode_boundaries_preserve_values_and_offsets() {
    for quote in ['\'', '"'] {
        let ascii: String = ('!'..='~')
            .filter(|&character| character != quote && (quote == '\'' || character != '\\'))
            .collect();
        // Single quotes allow literal backslashes and double quotes; double quotes
        // allow literal single quotes. Unicode interrupts and then resumes ASCII runs.
        let contents = format!(
            "{}é{}字{}🦀{}",
            ascii.repeat(30),
            ascii,
            ascii,
            ascii.repeat(30)
        );
        let prefix = "# é字\n[";
        let quoted = format!("{quote}{contents}{quote}");
        let source = format!("{prefix}{quoted}, tail]\n");
        let events = parse_all_inputs(&source);
        let (value, span) = scalar(&events);
        assert_eq!(value, &contents);
        assert!(matches!(value, Cow::Borrowed(_)));
        assert_eq!(span.start, Marker::new(prefix.chars().count(), 2, 1));
        assert_eq!(
            span.end.index(),
            prefix.chars().count() + quoted.chars().count()
        );
        assert_eq!(span.end.col(), 1 + quoted.chars().count());
        assert_eq!(
            span.byte_range(),
            Some(prefix.len()..prefix.len() + quoted.len())
        );
        assert_eq!(span.slice(&source), Some(quoted.as_str()));
    }
}

#[test]
fn escapes_preserve_long_prefixes_and_following_ascii_runs() {
    let run = "abcdef0123456789".repeat(256);
    for (quote, middle, decoded) in [
        ('\'', "''", "'"),
        ('\'', " \t''", " \t'"),
        ('"', "\\n", "\n"),
        ('"', " \t\\\"", " \t\""),
        ('"', "\\\\", "\\"),
        ('"', "\\u00e9", "é"),
        ('"', "\\uD83E\\uDD80", "🦀"),
    ] {
        let source = format!("[{quote}{run}{middle}{run}{quote}, tail]\n");
        let events = parse_all_inputs(&source);
        let (value, span) = scalar(&events);
        assert_eq!(value, &format!("{run}{decoded}{run}"));
        assert!(matches!(value, Cow::Owned(_)));
        assert_eq!(span.slice(&source), Some(&source[1..source.len() - 8]));
    }
}

#[test]
fn whitespace_and_crlf_folding_preserve_long_runs() {
    let run = "abcdefgh".repeat(256);
    for quote in ['\'', '"'] {
        for (middle, decoded, borrowed) in [
            (" \t ", " \t ", true),
            (" \t\r\n  ", " ", false),
            ("\r\n\r\n  ", "\n", false),
        ] {
            let source = format!("[{quote}{run}{middle}{run}{quote}, tail]\n");
            let events = parse_all_inputs(&source);
            let (value, _) = scalar(&events);
            assert_eq!(value, &format!("{run}{decoded}{run}"));
            assert_eq!(matches!(value, Cow::Borrowed(_)), borrowed);
        }
    }

    let source = format!("[\"{run}\\\r\n  {run}\", tail]\n");
    let events = parse_all_inputs(&source);
    let (value, span) = scalar(&events);
    assert_eq!(value, &run.repeat(2));
    assert!(matches!(value, Cow::Owned(_)));
    assert_eq!(span.end.line(), 2);
    assert_eq!(span.end.col(), run.len() + 3);
}

#[test]
fn controls_after_long_runs_are_rejected_at_the_exact_position() {
    let run = "x".repeat(4096);
    for quote in ['\'', '"'] {
        let controls = ('\0'..='\u{1f}')
            .filter(|character| !matches!(character, '\t' | '\r' | '\n'))
            .chain(['\u{7f}', '\u{80}', '\u{9f}', '\u{fffe}']);
        for character in controls {
            let prefix = format!("[{quote}é{run}");
            let source = format!("{prefix}{character}after{quote}]\n");
            let error = error_all_inputs(&source);
            let index = prefix.chars().count();
            assert_eq!(error.kind(), &ErrorKind::UnexpectedCharacter { character });
            assert_eq!(*error.marker(), Marker::new(index, 1, index));
            assert_eq!(error.marker().byte_offset(), Some(prefix.len()));
        }
    }
}

#[test]
fn unclosed_and_invalid_escaped_scalars_keep_the_opening_quote_marker() {
    let run = "x".repeat(4096);
    for quote in ['\'', '"'] {
        let source = format!("# é\n[{quote}{run}");
        let error = error_all_inputs(&source);
        assert_eq!(error.kind(), &ErrorKind::UnclosedQuotedScalar);
        assert_eq!(*error.marker(), Marker::new(5, 2, 1));
        assert_eq!(error.marker().byte_offset(), Some(6));
    }

    for (escape, expected) in [
        ("\\q", ErrorKind::UnknownQuotedScalarEscape),
        ("\\u12xz", ErrorKind::InvalidQuotedScalarHexEscape),
    ] {
        let source = format!("[\"{run}{escape}\"]");
        let error = error_all_inputs(&source);
        assert_eq!(error.kind(), &expected);
        assert_eq!(*error.marker(), Marker::new(1, 1, 1));
    }
}

#[test]
fn source_failures_after_long_quoted_runs_remain_terminal() {
    for quote in ['\'', '"'] {
        let source = format!("[{quote}{}", "x".repeat(4096));
        let expected = ErrorKind::InputIo {
            error: InputIoError::from_message("quoted scalar read failed"),
        };
        let input = source
            .chars()
            .map(Ok)
            .chain(std::iter::once(Err(expected.clone())))
            .chain(std::iter::from_fn(|| {
                panic!("source must not be polled after its first error")
            }));
        let error = terminal_error(Parser::new_from_fallible_iter(input));
        assert_eq!(error.kind(), &expected);
    }
}
