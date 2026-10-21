mod support;
#[cfg(feature = "comments")]
use granit_parser::Placement;
use granit_parser::{
    options, BufferedInput, ErrorKind, Event, Options, Parser, ScanError, Scanner, StrInput, Token,
    TokenType,
};

/// Drive the parser to completion and return the first error, if any.
fn first_error(yaml: &str, options: Options) -> Option<ScanError> {
    Parser::new_from_str_with_options(yaml, options).find_map(Result::err)
}

/// Drive an iterator-backed parser to completion and return the first error, if any.
fn first_iter_error(yaml: &str, options: Options) -> Option<ScanError> {
    Parser::new_from_iter_with_options(yaml.chars(), options).find_map(Result::err)
}

fn scanner_tokens(yaml: &str, options: Options) -> Vec<Token<'_>> {
    let tokens = Scanner::with_options(StrInput::new(yaml), options.clone())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let iter_tokens = Scanner::with_options(BufferedInput::new(yaml.chars()), options)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(tokens, iter_tokens, "input: {yaml:?}");
    tokens
}

#[test]
fn reserved_directive_separated_comments_preserve_parameters_and_scalar() {
    for (yaml, name, param, comment_text) in [
        (
            "%FUTURE option # keep this comment\n---\nvalue\n",
            "FUTURE",
            "option",
            " keep this comment",
        ),
        (
            "%FUTURE\toption\t# café 漢字\r\n---\r\nvalue\r\n",
            "FUTURE",
            "option",
            " café 漢字",
        ),
        (
            "%FUTURE#name option#value # actual comment\n---\nvalue\n",
            "FUTURE#name",
            "option#value",
            " actual comment",
        ),
    ] {
        #[cfg(not(feature = "comments"))]
        let _ = comment_text;
        let tokens = scanner_tokens(yaml, Options::default());
        assert_eq!(
            tokens[1].token_type(),
            &TokenType::ReservedDirective(name.into(), vec![param.into()]),
        );
        #[cfg(feature = "comments")]
        {
            let TokenType::Comment(comment) = tokens[2].token_type() else {
                panic!("expected a comment after the directive: {yaml:?}");
            };
            assert_eq!(comment.text(), comment_text);
            assert_eq!(comment.placement(), Placement::Right);
            assert_eq!(tokens[1].span().end, tokens[2].span().start);
            let comment_source = format!("#{comment_text}");
            assert_eq!(tokens[2].span().slice(yaml), Some(comment_source.as_str()));
        }

        let events = Parser::new_from_str(yaml)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            events,
            Parser::new_from_iter(yaml.chars())
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
        );
        #[cfg(feature = "comments")]
        {
            let comments = events
                .iter()
                .filter_map(|(event, span)| match event {
                    #[cfg(feature = "comments")]
                    Event::Comment(text, _) => Some((text.as_ref(), *span)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(comments, [(comment_text, tokens[2].span())]);
        }

        assert!(events.iter().any(|(event, span)| {
            matches!(event, Event::Scalar(value, ..) if value == "value")
                && span.slice(yaml) == Some("value")
        }));
    }
}

#[test]
fn reserved_directive_comments_do_not_consume_parameter_or_byte_limits() {
    for (directive, param_count) in [("FUTURE", 0), ("FUTURE option", 1)] {
        let yaml = format!("%{directive} #{}\n---\nvalue\n", " word".repeat(1000));
        for limits in [
            Options::default(),
            options! {
                max_reserved_directive_params: param_count,
                max_directive_bytes: directive.len(),
            },
        ] {
            for &emit_comments in support::comment_modes() {
                let options = limits.clone();
                #[cfg(feature = "comments")]
                let options = {
                    let mut options = options;
                    options.emit_comments = emit_comments;
                    options
                };
                let tokens = scanner_tokens(&yaml, options.clone());
                let TokenType::ReservedDirective(_, params) = tokens[1].token_type() else {
                    panic!("expected a reserved directive");
                };
                assert_eq!(params.len(), param_count);
                assert_eq!(
                    tokens
                        .iter()
                        .filter(|token| { support::is_comment_token(token.token_type()) })
                        .count(),
                    usize::from(emit_comments)
                );
                assert!(first_error(&yaml, options.clone()).is_none());
                assert!(first_iter_error(&yaml, options).is_none());
            }
        }
    }
}

#[test]
fn reserved_directive_comments_do_not_hide_excess_parameters() {
    for yaml in [
        "%FUTURE option # comment\n---\nvalue\n",
        "%FUTURE option#value # comment\n---\nvalue\n",
    ] {
        for &emit_comments in support::comment_modes() {
            let options = options! {
                max_reserved_directive_params: 0,
                #[cfg(feature = "comments")]
                emit_comments: emit_comments,
            };
            for error in [
                first_error(yaml, options.clone()),
                first_iter_error(yaml, options),
            ] {
                assert_eq!(
                    error.unwrap().kind(),
                    &ErrorKind::TooManyReservedDirectiveParams { limit: 0 },
                    "input: {yaml:?}, emit_comments: {emit_comments}",
                );
            }
        }
    }
}

#[test]
fn reserved_directive_comments_at_eof_do_not_supply_a_document() {
    for (directive, params) in [("FUTURE", vec![]), ("FUTURE option", vec!["option".into()])] {
        for comment_text in ["", " café 漢字"] {
            let yaml = format!("%{directive} #{comment_text}");
            for &emit_comments in support::comment_modes() {
                let options =
                    options! { #[cfg(feature = "comments")] emit_comments: emit_comments };
                let tokens = scanner_tokens(&yaml, options.clone());
                assert_eq!(
                    tokens[1].token_type(),
                    &TokenType::ReservedDirective("FUTURE".into(), params.clone()),
                );
                assert_eq!(tokens.len(), 3 + usize::from(emit_comments));
                assert_eq!(tokens.last().unwrap().token_type(), &TokenType::StreamEnd);
                #[cfg(feature = "comments")]
                if emit_comments {
                    let TokenType::Comment(comment) = tokens[2].token_type() else {
                        panic!("expected EOF comment: {yaml:?}");
                    };
                    assert_eq!(comment.text(), comment_text);
                    assert_eq!(comment.placement(), Placement::Right);
                    assert_eq!(
                        tokens[2].span().slice(&yaml),
                        Some(&yaml[1 + directive.len() + 1..])
                    );
                }
                for error in [
                    first_error(&yaml, options.clone()),
                    first_iter_error(&yaml, options),
                ] {
                    assert_eq!(error.unwrap().kind(), &ErrorKind::ExpectedDocumentStart);
                }
            }
        }
    }
}

#[test]
fn reserved_directive_comments_still_validate_control_characters_when_suppressed() {
    for character in ['\u{1}', '\u{7f}'] {
        let yaml = format!("%FUTURE option # café {character}\n---\nvalue\n");
        for &emit_comments in support::comment_modes() {
            #[cfg(not(feature = "comments"))]
            let _ = emit_comments;
            let options = options! { #[cfg(feature = "comments")] emit_comments: emit_comments };
            for error in [
                first_error(&yaml, options.clone()),
                first_iter_error(&yaml, options),
            ] {
                let error = error.expect("invalid comment content must be rejected");
                assert_eq!(error.kind(), &ErrorKind::UnexpectedCharacter { character });
                assert_eq!(error.marker().line(), 1);
                assert_eq!(
                    error.marker().col(),
                    "%FUTURE option # café ".chars().count()
                );
            }
        }
    }
}

#[test]
fn reserved_directive_comment_separator_is_excluded_from_utf8_byte_limit() {
    for directive in ["FUTURE\toption#value", "FÜTURE é"] {
        let yaml = format!("%{directive}{}# comment\n---\nvalue\n", " \t".repeat(1024));
        for &emit_comments in support::comment_modes() {
            let exact = options! {
                max_directive_bytes: directive.len(),
                max_reserved_directive_params: 1,
                #[cfg(feature = "comments")]
                emit_comments: emit_comments,
            };
            let tokens = scanner_tokens(&yaml, exact.clone());
            assert!(
                matches!(tokens[1].token_type(), TokenType::ReservedDirective(_, params) if params.len() == 1)
            );
            assert!(first_error(&yaml, exact.clone()).is_none());
            assert!(first_iter_error(&yaml, exact.clone()).is_none());

            let mut too_small = exact;
            too_small.max_directive_bytes -= 1;
            for error in [
                first_error(&yaml, too_small.clone()),
                first_iter_error(&yaml, too_small),
            ] {
                assert_eq!(
                    error.unwrap().kind(),
                    &ErrorKind::DirectiveByteLimitExceeded {
                        limit: directive.len() - 1
                    },
                    "input directive: {directive:?}, emit_comments: {emit_comments}",
                );
            }
        }
    }
}

// ZYU8: Directive variants
// In YAML 1.2, a directive name is any non\u{2011}space, non\u{2011}line\u{2011}break sequence of characters
// Saphyr expects only alphabetic characters in a directive name, dot . triggers the error.
#[test]
fn yaml_zyu8_directive_variant_yaml11_null_document() {
    let yaml = "%YAML1.1\n---\n";
    let mut got_err: Option<ScanError> = None;

    for item in Parser::new_from_str(yaml) {
        match item {
            Ok((_event, _span)) => {}
            Err(e) => {
                got_err = Some(e);
                break;
            }
        }
    }
    assert!(got_err.is_none(), "Error: {}", got_err.unwrap().info());
}

#[test]
fn yaml_reserved_directive_stars() {
    let yaml = "%***\n---\n";
    let mut got_err: Option<ScanError> = None;

    for item in Parser::new_from_str(yaml) {
        if let Err(e) = item {
            got_err = Some(e);
            break;
        }
    }
    assert!(got_err.is_none(), "Error: {}", got_err.unwrap().info());
}

#[test]
fn yaml_bad_yaml_directive() {
    let yaml = "%YAML 1.1 1.2\n---\n";
    let mut got_err: Option<ScanError> = None;

    for item in Parser::new_from_str(yaml) {
        if let Err(e) = item {
            got_err = Some(e);
            break;
        }
    }
    // This should fail because "YAML" is a defined directive and it has too many parameters.
    assert!(got_err.is_some());
    assert!(got_err
        .unwrap()
        .info()
        .contains("did not find expected comment or line break"));
}

#[test]
fn yaml_reserved_directive_with_params() {
    let yaml = "%FOO bar baz\n---\n";
    let mut got_err: Option<ScanError> = None;

    for item in Parser::new_from_str(yaml) {
        if let Err(e) = item {
            got_err = Some(e);
            break;
        }
    }
    assert!(got_err.is_none(), "Error: {}", got_err.unwrap().info());
}

#[test]
fn yaml_reserved_directive_at_eof() {
    let yaml = "%FOO";
    let mut got_err: Option<ScanError> = None;

    for item in Parser::new_from_str(yaml) {
        if let Err(e) = item {
            got_err = Some(e);
            break;
        }
    }
    assert!(got_err.is_some(), "Expected an error");
    assert!(got_err
        .unwrap()
        .info()
        .contains("did not find expected <document start>"));
}

#[test]
fn yaml_reserved_directive_with_param_at_eof() {
    let yaml = "%FOO bar";
    let mut got_err: Option<ScanError> = None;

    for item in Parser::new_from_str(yaml) {
        if let Err(e) = item {
            got_err = Some(e);
            break;
        }
    }
    assert!(got_err.is_some(), "Expected an error");
    assert!(got_err
        .unwrap()
        .info()
        .contains("did not find expected <document start>"));
}

// The parser ignores reserved directives, but the scanner still has to materialize one `String`
// per parameter to build the token. Without a limit, `%X` followed by two-byte ` a` parameters
// buys an allocation and a vector slot for every two input bytes, all inside a single token and
// therefore before any event-level budget can see it.
#[test]
fn reserved_directive_params_are_capped_by_default() {
    let mut yaml = String::from("%X");
    for _ in 0..100_000 {
        yaml.push_str(" a");
    }
    yaml.push_str("\n---\n");

    let err = first_error(&yaml, Options::default()).expect("expected a limit error");
    assert!(
        err.info().contains("reserved directive exceeds"),
        "unexpected error: {}",
        err.info()
    );
}

#[test]
fn reserved_directive_param_count_limit_is_configurable() {
    let options = options! { max_reserved_directive_params: 2 };

    assert!(first_error("%X a b\n---\n", options.clone()).is_none());

    let err = first_error("%X a b c\n---\n", options).expect("expected a limit error");
    assert!(
        err.info()
            .contains("reserved directive exceeds the configured limit of 2 parameters"),
        "unexpected error: {}",
        err.info()
    );
}

// A single unbounded parameter is a weaker amplification than many small ones, but the scanner
// would still retain a `String` the size of the input for a token the parser discards.
#[test]
fn oversized_reserved_directive_param_is_rejected() {
    let mut yaml = String::from("%X ");
    yaml.push_str(&"a".repeat(100_000));
    yaml.push_str("\n---\n");

    let err = first_error(&yaml, Options::default()).expect("expected a limit error");
    assert!(
        err.info()
            .contains("directive exceeds the configured limit of 1024 bytes"),
        "unexpected error: {}",
        err.info()
    );
}

#[test]
fn oversized_directive_name_is_rejected() {
    let yaml = format!("%{}\n---\n", "A".repeat(100_000));

    let err = first_error(&yaml, Options::default()).expect("expected a limit error");
    assert!(
        err.info()
            .contains("directive exceeds the configured limit of 1024 bytes"),
        "unexpected error: {}",
        err.info()
    );
}

// The byte limit spans the whole directive, so parameters that individually fit still cannot add
// up to an unbounded token.
#[test]
fn reserved_directive_bytes_are_capped_across_params() {
    let options = options! { max_reserved_directive_params: usize::MAX };
    let mut yaml = String::from("%X");
    for _ in 0..100_000 {
        yaml.push_str(" aaaaaaaa");
    }
    yaml.push_str("\n---\n");

    let err = first_error(&yaml, options).expect("expected a limit error");
    assert!(
        err.info()
            .contains("directive exceeds the configured limit of 1024 bytes"),
        "unexpected error: {}",
        err.info()
    );
}

#[test]
fn directive_byte_limit_is_configurable_and_counts_multibyte_chars() {
    let options = options! { max_directive_bytes: 8 };

    // The name, the separating blank and three two-byte characters exactly fill the budget.
    assert!(first_error("%X ÿÿÿ\n---\n", options.clone()).is_none());

    let err = first_error("%X ÿÿÿÿ\n---\n", options).expect("expected a limit error");
    assert!(
        err.info()
            .contains("directive exceeds the configured limit of 8 bytes"),
        "unexpected error: {}",
        err.info()
    );
}

// Real directives are short; the defaults must not disturb them.
#[test]
fn ordinary_directives_are_unaffected_by_the_limits() {
    let yaml = "%YAML 1.2\n%TAG !e! tag:example.com,2000:app/\n%FOO bar baz\n---\nkey: value\n";

    assert!(first_error(yaml, Options::default()).is_none());
}

#[test]
fn tag_directive_handle_and_prefix_are_capped_for_borrowed_and_streaming_inputs() {
    let oversized = [
        format!("%TAG !{}! x\n---\n", "a".repeat(100_000)),
        format!("%TAG !e! tag:{}\n---\n", "a".repeat(100_000)),
    ];

    for yaml in &oversized {
        for error in [
            first_error(yaml, Options::default()),
            first_iter_error(yaml, Options::default()),
        ] {
            let error = error.expect("expected a directive byte-limit error");
            assert_eq!(
                error.kind(),
                &ErrorKind::DirectiveByteLimitExceeded { limit: 1024 }
            );
        }
    }
}

#[test]
fn tag_directive_byte_limit_counts_separators_and_raw_escape_bytes() {
    let yaml = "%TAG !e! %C3%BF\n---\n";
    let exact = options! { max_directive_bytes: 14 };

    assert!(first_error(yaml, exact.clone()).is_none());
    assert!(first_iter_error(yaml, exact).is_none());

    let too_small = options! { max_directive_bytes: 13 };
    for error in [
        first_error(yaml, too_small.clone()),
        first_iter_error(yaml, too_small),
    ] {
        let error = error.expect("expected a directive byte-limit error");
        assert_eq!(
            error.kind(),
            &ErrorKind::DirectiveByteLimitExceeded { limit: 13 }
        );
    }
}
