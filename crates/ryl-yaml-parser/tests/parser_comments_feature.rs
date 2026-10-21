#![cfg(not(feature = "comments"))]

use granit_parser::{
    BufferedInput, ErrorKind, Event, FallibleBufferedInput, Marker, Parser, ParserStack,
    ScalarStyle, Scanner, Span, StrInput, TokenType,
};

fn parsed_events(yaml: &str) -> Vec<(Event<'_>, Span)> {
    let events = Parser::new_from_str(yaml)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        events,
        Parser::new_from_iter(yaml.chars())
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        "{yaml:?}"
    );
    events
}

fn assert_monotonic_spans(events: &[(Event<'_>, Span)]) {
    for pair in events.windows(2) {
        assert!(
            pair[1].1.start.index() >= pair[0].1.start.index()
                && pair[1].1.end.index() >= pair[0].1.end.index(),
            "event spans moved backwards: {pair:?}"
        );
    }
}

#[test]
fn default_scanners_and_parsers_ignore_comments_for_all_input_backends() {
    for yaml in [
        "# header\nroot: # empty\nnext: value # trailing\n",
        "root:\n- # empty entry\n- value # trailing\n",
        "{root: # empty flow value\n, next: value}\n",
        "--- # document start\nvalue\n... # document end\n# tail\n",
    ] {
        let string = Parser::new_from_str(yaml)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let buffered = Parser::new_from_iter(yaml.chars())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let fallible = Parser::new_from_fallible_iter(yaml.chars().map(Ok::<char, ErrorKind>))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(string, buffered, "{yaml:?}");
        assert_eq!(string, fallible, "{yaml:?}");
        assert!(string
            .iter()
            .any(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == "value")));

        let string = Scanner::new(StrInput::new(yaml))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let buffered = Scanner::new(BufferedInput::new(yaml.chars()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let fallible = Scanner::new(FallibleBufferedInput::new(
            yaml.chars().map(Ok::<char, ErrorKind>),
        ))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
        assert_eq!(string, buffered, "{yaml:?}");
        assert_eq!(string, fallible, "{yaml:?}");
        assert!(string.iter().any(|token| {
            matches!(token.token_type(), TokenType::Scalar(_, value) if value == "value")
        }));
    }
}

#[test]
fn excluded_comments_preserve_syntax_validation() {
    for (yaml, expected) in [
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
            "key: value # control \u{1}\n",
            ErrorKind::UnexpectedCharacter { character: '\u{1}' },
        ),
        ("?\n\tkey\n", ErrorKind::TabNotAllowed),
    ] {
        let string = Parser::new_from_str(yaml).find_map(Result::err).unwrap();
        let buffered = Parser::new_from_iter(yaml.chars())
            .find_map(Result::err)
            .unwrap();
        let fallible = Parser::new_from_fallible_iter(yaml.chars().map(Ok::<char, ErrorKind>))
            .find_map(Result::err)
            .unwrap();
        assert_eq!(string.kind(), &expected, "{yaml:?}");
        assert_eq!(string, buffered, "{yaml:?}");
        assert_eq!(string, fallible, "{yaml:?}");
        assert_eq!(
            Scanner::new(StrInput::new(yaml))
                .find_map(Result::err)
                .unwrap(),
            string,
            "{yaml:?}",
        );
    }
}

#[test]
fn ignored_unicode_comments_preserve_following_token_and_event_positions() {
    let yaml = "# café 🪨\r\nclé: valeur # fin 漢字\r\nnext: value\r\n";
    let byte_offset = yaml.find("next").unwrap();
    let character_offset = yaml[..byte_offset].chars().count();
    let tokens = Scanner::new(StrInput::new(yaml))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let token = tokens
        .iter()
        .find(|token| matches!(token.token_type(), TokenType::Scalar(_, value) if value == "next"))
        .unwrap();
    let events = Parser::new_from_str(yaml)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let (_, span) = events
        .iter()
        .find(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == "next"))
        .unwrap();
    for span in [token.span(), *span] {
        assert_eq!(span.start.index(), character_offset);
        assert_eq!(span.start.byte_offset(), Some(byte_offset));
        assert_eq!((span.start.line(), span.start.col()), (3, 0));
        assert_eq!(span.slice(yaml), Some("next"));
    }
}

#[test]
fn excluded_comments_keep_hashes_inside_scalars_and_nested_sources() {
    type Stack = ParserStack<'static, core::iter::Empty<char>, StrInput<'static>>;

    let yaml = "# ignored\n[plain#hash, \"# quoted\", '# single'] # ignored\n";
    let values = Parser::new_from_str(yaml)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .filter_map(|(event, _)| match event {
            Event::Scalar(value, ..) => Some(value.into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(values, ["plain#hash", "# quoted", "# single"]);

    let mut stack = Stack::new();
    stack.push_str_parser(Parser::new_from_str("parent\n"), "parent".into());
    stack.push_str_parser(
        Parser::new_from_str("# header\nchild\n... # tail\n# after\n"),
        "child".into(),
    );
    assert_eq!(
        stack
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .map(|(event, _)| event)
            .collect::<Vec<_>>(),
        [
            Event::Scalar("child".into(), ScalarStyle::Plain, 0, None),
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::Scalar("parent".into(), ScalarStyle::Plain, 0, None),
            Event::DocumentEnd,
            Event::StreamEnd,
        ],
    );
}

#[test]
fn ignored_comments_preserve_empty_node_spans() {
    for (yaml, expected) in [
        ("key: # c\nnext: v\n", Marker::new(3, 1, 3)),
        ("- # c\n- v\n", Marker::new(5, 1, 5)),
        ("key:\n- # c\n- v\n", Marker::new(10, 2, 5)),
        ("{key: # c\n}", Marker::new(4, 1, 4)),
    ] {
        let events = parsed_events(yaml);
        let (_, span) = events
            .iter()
            .find(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == "~"))
            .expect("comment-separated empty node should be emitted");
        assert_eq!(span.start, expected, "{yaml:?}");
        assert_eq!(span.end, expected, "{yaml:?}");
        assert_eq!(span.start.byte_offset(), Some(expected.index()), "{yaml:?}");
        assert_eq!(span.slice(yaml), Some(""), "{yaml:?}");
        assert_monotonic_spans(&events);
    }
}

#[test]
fn ignored_comment_runs_preserve_span_order() {
    let comments = "# later\n".repeat(95);
    for yaml in [
        "plain key: in-line value\n: # Both empty\n\"quoted key\":\n- entry\n".into(),
        format!("key: # c0\n{comments}next: value\n"),
        format!("- # c0\n{comments}- value\n"),
        format!("key:\n- # c0\n{comments}next: value\n"),
        format!("root: {{key: # c0\n{comments}}}\n"),
    ] {
        assert_monotonic_spans(&parsed_events(&yaml));
    }
}

#[test]
fn ignored_comments_preserve_collection_entry_order() {
    for (yaml, expected) in [
        (
            "key:\n- # value\n  first\nnext: value\n",
            ["key", "first", "<sequence end>", "next", "value"].as_slice(),
        ),
        (
            "key:\n- first\n- # value\n  second\nnext: value\n",
            ["key", "first", "second", "<sequence end>", "next", "value"].as_slice(),
        ),
        (
            "key:\n- first\n- # empty\nnext: value\n",
            ["key", "first", "~", "<sequence end>", "next", "value"].as_slice(),
        ),
        (
            "{? # key\n  key\n: value, # comma\nnext: value}\n",
            ["key", "value", "next", "value"].as_slice(),
        ),
        ("{key: # value\n value}\n", ["key", "value"].as_slice()),
        (
            "[key: # value\n value]\n",
            ["key", "value", "<sequence end>"].as_slice(),
        ),
    ] {
        let events = parsed_events(yaml);
        let values = events
            .iter()
            .filter_map(|(event, _)| match event {
                Event::Scalar(value, ScalarStyle::Plain, ..) => Some(value.as_ref()),
                Event::SequenceEnd => Some("<sequence end>"),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(values, expected, "{yaml:?}");
    }
}

#[test]
fn ignored_comments_preserve_complementary_node_properties() {
    for yaml in ["&a # anchor\n!t value\n", "!t # tag\n&a value\n"] {
        let events = parsed_events(yaml);
        let (anchor_id, tag) = events
            .iter()
            .find_map(|(event, _)| match event {
                Event::Scalar(value, _, anchor_id, Some(tag)) if value == "value" => {
                    Some((*anchor_id, tag.original()))
                }
                _ => None,
            })
            .expect("comment-separated value should retain both node properties");
        assert_ne!(anchor_id, 0, "{yaml:?}");
        assert_eq!(tag, "!t", "{yaml:?}");
    }
}

#[test]
fn ignored_anchor_comments_preserve_aliases_and_self_reference() {
    for yaml in [
        "&a # anchor\n[*a]\n",
        "? # key\n: &a # anchor\n  value\nref: *a # alias\n",
    ] {
        let events = parsed_events(yaml);
        let anchor_id = events
            .iter()
            .find_map(|(event, _)| event.anchor_id().filter(|id| *id != 0))
            .expect("comment-separated node should retain its anchor");
        assert!(
            events.iter().any(|(event, _)| {
                matches!(event, Event::Alias(alias_id) if *alias_id == anchor_id)
            }),
            "{yaml:?}"
        );
    }
}
