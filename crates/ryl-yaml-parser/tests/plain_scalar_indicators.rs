use granit_parser::{
    BufferedInput, ErrorKind, Event, Parser, ScalarStyle, ScanError, Scanner, StrInput,
    StructureStyle, TokenType,
};

// Regression guards for StrInput::next_can_be_plain_scalar simplification.
// YAML 1.2 7.3.3: indicator characters can end a plain scalar in certain positions.

#[test]
fn colon_followed_by_space_ends_plain_scalar_not_part_of_value() {
    // After "foo:", seeing "a: b" means the colon after 'a' is treated as YAML syntax,
    // not as part of the plain scalar. Without a newline/indentation or flow braces,
    // this input is invalid in YAML and our parser must error (it must NOT accept
    // "a: b" as a single scalar value).
    let s = "foo: a: b\n";

    let it = Parser::new_from_str(s);
    let mut got_err: Option<ScanError> = None;
    for res in it {
        if let Err(e) = res {
            got_err = Some(e);
            break;
        }
    }
    let err = got_err.expect("no error on inline nested mapping without indentation");
    assert_eq!(err.info(), "mapping values are not allowed in this context");
}

#[test]
fn colon_without_space_is_part_of_scalar_value() {
    // When there is no blank after ':', the ':' is part of the plain scalar (7.3.3).
    // Here the value should be the single scalar "a:b".
    let s = "k: a:b\n";
    let events: Vec<_> = Parser::new_from_str(s).map(|r| r.unwrap().0).collect();
    assert_eq!(
        events,
        vec![
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar("k".into(), ScalarStyle::Plain, 0, None),
            Event::Scalar("a:b".into(), ScalarStyle::Plain, 0, None),
            Event::MappingEnd,
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

#[test]
fn plain_scalar_dash_before_flow_delimiter() {
    for (yaml, expected_first) in [
        ("[a -, after]", "a -"),
        ("[\u{fffd} -, after]", "\u{fffd} -"),
        ("[a-, after]", "a-"),
        ("[a\t-, after]", "a\t-"),
        ("[a\n -, after]", "a -"),
    ] {
        for result in [
            Parser::new_from_str(yaml).collect::<Result<Vec<_>, _>>(),
            Parser::new_from_iter(yaml.chars()).collect::<Result<Vec<_>, _>>(),
        ] {
            let events: Vec<_> = result
                .unwrap_or_else(|error| panic!("input: {yaml:?}: {error}"))
                .into_iter()
                .map(|(event, _)| event)
                .collect();
            assert_eq!(
                events,
                [
                    Event::StreamStart,
                    Event::DocumentStart(false, None),
                    Event::SequenceStart(StructureStyle::Flow, 0, None),
                    Event::Scalar(expected_first.into(), ScalarStyle::Plain, 0, None),
                    Event::Scalar("after".into(), ScalarStyle::Plain, 0, None),
                    Event::SequenceEnd,
                    Event::DocumentEnd,
                    Event::StreamEnd,
                ],
                "input: {yaml:?}",
            );
        }
    }
}

#[test]
fn plain_scalar_dash_is_included_in_token_before_flow_delimiter() {
    for first in ["a -", "\u{fffd} -"] {
        let yaml = format!("[{first}, after]");
        for result in [
            Scanner::new(StrInput::new(&yaml)).collect::<Result<Vec<_>, _>>(),
            Scanner::new(BufferedInput::new(yaml.chars())).collect::<Result<Vec<_>, _>>(),
        ] {
            let tokens = result.unwrap_or_else(|error| panic!("input: {yaml:?}: {error}"));
            let types: Vec<_> = tokens
                .iter()
                .map(|token| token.token_type().clone())
                .collect();
            assert_eq!(
                types,
                [
                    TokenType::StreamStart,
                    TokenType::FlowSequenceStart,
                    TokenType::Scalar(ScalarStyle::Plain, first.into()),
                    TokenType::FlowEntry,
                    TokenType::Scalar(ScalarStyle::Plain, "after".into()),
                    TokenType::FlowSequenceEnd,
                    TokenType::StreamEnd,
                ],
                "input: {yaml:?}",
            );
            assert_eq!(tokens[2].span().end, tokens[3].span().start);
        }
    }
}

#[test]
fn plain_scalar_dash_before_closing_flow_delimiter() {
    for yaml in ["[a -]", "{key: a -}"] {
        for result in [
            Parser::new_from_str(yaml).collect::<Result<Vec<_>, _>>(),
            Parser::new_from_iter(yaml.chars()).collect::<Result<Vec<_>, _>>(),
        ] {
            let events = result.unwrap_or_else(|error| panic!("input: {yaml:?}: {error}"));
            assert!(events.iter().any(|(event, _)| {
                matches!(event, Event::Scalar(value, ScalarStyle::Plain, ..) if value == "a -")
            }));
        }
    }
}

#[test]
fn plain_scalar_cannot_start_with_dash_before_flow_delimiter() {
    for yaml in ["[-, after]", "[-[]]", "[-]", "[-{}]", "{key: -}"] {
        for result in [
            Parser::new_from_str(yaml).collect::<Result<Vec<_>, _>>(),
            Parser::new_from_iter(yaml.chars()).collect::<Result<Vec<_>, _>>(),
        ] {
            let error = result.expect_err(yaml);
            assert_eq!(
                error.kind(),
                &ErrorKind::PlainScalarStartsWithDashFlowIndicator,
                "input: {yaml:?}",
            );
        }
    }
}
