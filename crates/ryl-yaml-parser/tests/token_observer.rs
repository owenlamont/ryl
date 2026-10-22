use ryl_yaml_parser::{
    BufferedInput, Parser, Scanner, Span, StrInput, TokenKind, options,
};

fn observed(text: &str) -> Vec<(Span, String)> {
    let mut tokens = Vec::new();
    let mut parser = Parser::with_options_and_observer(
        StrInput::new(text),
        options! { strict_indentation: true },
        |span: &Span, kind: TokenKind<'_>| tokens.push((*span, format!("{kind:?}"))),
    );
    while parser.peek().is_some() {
        let before = parser.peek().unwrap().unwrap().clone();
        assert_eq!(parser.next().unwrap().unwrap(), before);
    }
    tokens
}

#[test]
fn observer_matches_settled_scanner_order_with_synthetic_keys_and_comments() {
    let text =
        "%YAML 1.2\n---\n# heading\n? [é, 🦀]\n: {a: &a 'x', b: *a} # tail\n...\n";
    let actual = observed(text);
    let expected: Vec<_> = Scanner::new(StrInput::new(text))
        .map(|token| {
            let token = token.unwrap();
            (token.span(), format!("{:?}", token.token_type().kind()))
        })
        .collect();
    assert_eq!(actual, expected);
    assert!(actual.last().unwrap().1.contains("StreamEnd"));
    let mut missing = actual.clone();
    missing.pop();
    assert_ne!(missing, expected);
    let mut reordered = actual;
    reordered.swap(1, 2);
    assert_ne!(reordered, expected);
}

#[test]
fn buffered_observation_preserves_character_coordinates_and_decoded_values() {
    let text = "café: [\"a\\nb\", 世]\r\nnext: >-\r  folded\r  text\r";
    let expected = observed(text);
    let mut actual = Vec::new();
    let parser = Parser::with_options_and_observer(
        BufferedInput::new(text.chars()),
        options! { strict_indentation: true },
        |span: &Span, kind: TokenKind<'_>| actual.push((*span, format!("{kind:?}"))),
    );
    assert!(parser.collect::<Result<Vec<_>, _>>().is_ok());
    assert_eq!(actual, expected);
    assert!(
        actual
            .iter()
            .all(|(span, _)| span.start.byte_offset().is_none())
    );
}

#[test]
fn failed_parse_has_no_completion_token_and_keeps_the_parser_error() {
    let text = "a: *missing\nb: ['unfinished\n";
    let mut tokens = Vec::new();
    let actual = Parser::with_options_and_observer(
        StrInput::new(text),
        options! { strict_indentation: true },
        |_: &Span, kind: TokenKind<'_>| tokens.push(format!("{kind:?}")),
    )
    .collect::<Result<Vec<_>, _>>();
    let expected =
        Parser::new_from_str_with_options(text, options! { strict_indentation: true })
            .collect::<Result<Vec<_>, _>>();
    assert_eq!(actual, expected);
    assert!(actual.is_err());
    assert!(!tokens.iter().any(|kind| kind == "StreamEnd"));
}

#[test]
fn late_scanner_error_preserves_observed_prefix_without_completion() {
    let text = "a: b\n...\n# before error\n---\nbad: [\n";
    let mut actual = Vec::new();
    let result = Parser::with_options_and_observer(
        StrInput::new(text),
        options! { strict_indentation: true },
        |span: &Span, kind: TokenKind<'_>| actual.push((*span, format!("{kind:?}"))),
    )
    .collect::<Result<Vec<_>, _>>();
    assert!(result.is_err());
    let expected: Vec<_> = Scanner::new(StrInput::new(text))
        .map_while(Result::ok)
        .map(|token| (token.span(), format!("{:?}", token.token_type().kind())))
        .collect();
    assert_eq!(actual, expected);
    assert!(!actual.iter().any(|(_, kind)| kind == "StreamEnd"));
}
