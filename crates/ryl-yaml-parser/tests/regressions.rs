use granit_parser::{
    BufferedInput, ErrorKind, Event, Options, Parser, ScalarStyle, ScanError, Scanner, Span,
    StrInput, StructureStyle,
};
use std::collections::BTreeSet;

fn collect_ok_events(yaml: &str) -> Vec<(Event<'_>, Span)> {
    Parser::new_from_str(yaml)
        .map(|result| result.expect("regression input should parse"))
        .collect()
}

fn first_error(input: &str) -> ScanError {
    Parser::new_from_str(input)
        .find_map(Result::err)
        .expect("regression input should produce an error")
}

fn scalar_values(input: &str) -> Vec<String> {
    collect_ok_events(input)
        .into_iter()
        .filter_map(|(event, _)| match event {
            Event::Scalar(value, ..) => Some(value.into_owned()),
            _ => None,
        })
        .collect()
}

fn scalar_values_with_style(input: &str, style: ScalarStyle) -> Vec<String> {
    collect_ok_events(input)
        .into_iter()
        .filter_map(|(event, _)| match event {
            Event::Scalar(value, scalar_style, ..) if scalar_style == style => {
                Some(value.into_owned())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn block_scalar_indicators_cannot_start_nodes_in_flow_collections() {
    fn assert_terminal_error<T>(
        mut items: impl Iterator<Item = Result<T, ScanError>>,
        yaml: &str,
        indicator: char,
    ) {
        let error = items
            .by_ref()
            .find_map(Result::err)
            .unwrap_or_else(|| panic!("expected an error for {yaml:?}"));
        assert_eq!(
            error.kind(),
            &ErrorKind::UnexpectedCharacter {
                character: indicator
            },
            "input: {yaml:?}",
        );
        let offset = yaml.find(indicator).unwrap();
        let prefix = &yaml[..offset];
        assert_eq!(error.marker().index(), prefix.chars().count());
        assert_eq!(
            error.marker().line(),
            prefix.bytes().filter(|&byte| byte == b'\n').count() + 1
        );
        assert_eq!(
            error.marker().col(),
            prefix.rsplit('\n').next().unwrap().chars().count()
        );
        if let Some(byte_offset) = error.marker().byte_offset() {
            assert_eq!(byte_offset, offset);
        }
        assert!(items.next().is_none(), "error must be terminal: {yaml:?}");
        assert!(items.next().is_none(), "iterator must stay fused: {yaml:?}");
    }

    for indicator in ['|', '>'] {
        for yaml in [
            format!("[{indicator}]\n"),
            format!("[{indicator}text]\n"),
            format!("{{{indicator}: value}}\n"),
            format!("{{key: {indicator}}}\n"),
            format!("[key:\t{indicator}]\n"),
            format!("[&anchor {indicator}]\n"),
            format!("[!tag {indicator}]\n"),
            format!("{{outer: [pré,\r\n  {indicator}text]}}\n"),
        ] {
            assert_terminal_error(Parser::new_from_str(&yaml), &yaml, indicator);
            assert_terminal_error(Parser::new_from_iter(yaml.chars()), &yaml, indicator);
            assert_terminal_error(Scanner::new(StrInput::new(&yaml)), &yaml, indicator);
            assert_terminal_error(
                Scanner::new(BufferedInput::new(yaml.chars())),
                &yaml,
                indicator,
            );
        }
    }
}

#[test]
fn block_scalar_indicator_characters_remain_valid_in_scalar_content() {
    assert_eq!(
        scalar_values("[a|b, a>b, \"|\", '>']\n"),
        ["a|b", "a>b", "|", ">"],
    );

    for (yaml, expected) in [
        ("[a|b, a>b]\n", ["a|b", "a>b"]),
        ("[a | b, a > b]\n", ["a | b", "a > b"]),
        ("[a\n |b, a\n >b]\n", ["a |b", "a >b"]),
        ("{a|b: c>d}\n", ["a|b", "c>d"]),
        ("[é|b, 字>b]\n", ["é|b", "字>b"]),
    ] {
        let str_events = Parser::new_from_str(yaml)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let iter_events = Parser::new_from_iter(yaml.chars())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let fallible_events = Parser::new_from_fallible_iter(yaml.chars().map(Ok::<_, ErrorKind>))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(str_events, iter_events, "input: {yaml:?}");
        assert_eq!(str_events, fallible_events, "input: {yaml:?}");
        let scalars: Vec<_> = str_events
            .iter()
            .filter_map(|(event, _)| match event {
                Event::Scalar(value, style, ..) => Some((value.as_ref(), *style)),
                _ => None,
            })
            .collect();
        assert_eq!(
            scalars,
            expected.map(|value| (value, ScalarStyle::Plain)),
            "input: {yaml:?}"
        );
    }
}

#[test]
fn block_scalars_remain_valid_outside_flow_collections() {
    assert_eq!(
        scalar_values_with_style("key: |\n  literal\n", ScalarStyle::Literal),
        ["literal\n"],
    );
    assert_eq!(
        scalar_values_with_style("key: >\n  folded\n", ScalarStyle::Folded),
        ["folded\n"],
    );
}

#[test]
fn alias_anchor_edge_cases() {
    assert_eq!(
        first_error("a: *nope\n").info(),
        "while parsing node, found unknown anchor"
    );
    assert_eq!(
        first_error("--- &x 1\n--- *x\n").info(),
        "while parsing node, found unknown anchor"
    );

    let self_reference = collect_ok_events("&x [*x]\n");
    assert!(self_reference
        .iter()
        .any(|(event, _)| matches!(event, Event::SequenceStart(StructureStyle::Flow, 1, None))));
    assert!(self_reference
        .iter()
        .any(|(event, _)| matches!(event, Event::Alias(1))));

    let redefinition = collect_ok_events("[&x 1, &x 2, *x]\n");
    assert!(redefinition.iter().any(|(event, _)| matches!(
        event,
        Event::Scalar(value, _, 1, _) if value.as_ref() == "1"
    )));
    assert!(redefinition.iter().any(|(event, _)| matches!(
        event,
        Event::Scalar(value, _, 2, _) if value.as_ref() == "2"
    )));
    assert!(redefinition
        .iter()
        .any(|(event, _)| matches!(event, Event::Alias(2))));
}

fn assert_emitted_aliases_reference_preceding_anchor_nodes(yaml: &str) {
    let mut anchors = BTreeSet::new();
    for result in Parser::new_from_str(yaml) {
        let Ok((event, _)) = result else {
            // Rejecting malformed YAML is valid, but every event emitted before the error must
            // still satisfy the alias invariant.
            return;
        };

        if matches!(event, Event::DocumentStart(..)) {
            anchors.clear();
        }

        if let Some(anchor) = event.anchor_id() {
            anchors.insert(anchor);
        }

        if let Event::Alias(anchor) = event {
            assert!(
                anchors.contains(&anchor),
                "alias {anchor} was emitted before its anchor node for {yaml:?}"
            );
        }
    }
}

#[test]
fn anchor_without_node_does_not_make_later_alias_valid() {
    // Minimized from the flow_collections fuzz failure.
    assert_emitted_aliases_reference_preceding_anchor_nodes("[k: &a #\n*a]\n");
}

#[test]
fn overwritten_pending_anchor_does_not_make_later_alias_valid() {
    // Minimized from the directives_tags fuzz failures. The second anchor replaces
    // the pending first anchor before the scalar event is emitted.
    let yaml = "node:\n  &a !t #\n  &b value\nalias: *a\n";
    assert_emitted_aliases_reference_preceding_anchor_nodes(yaml);
}

#[test]
fn directives_tags_fuzz_crash_is_expected_directive_limit() {
    const INPUT: &[u8] = concat!(
        "x\0\0\0- name: Eted\n",
        "  tags: edge\n",
        "  -MAP\n",
        "     -DOC\n",
        "    -STR\n",
        "  emit: |\n",
        "    ---\n",
        "- name: Comment TR\n",
        "  emit: |\n",
        "    ---\n",
        "- name: Comment that looks like a mapping key\n",
        "  fr:om: '@perlpunk'\n",
        "  tags: comment error mapping\n",
        "  fail: true\n",
        "  yaml: |\n",
        "    key: value\n",
        "    this is #not \n",
        "a: key\n",
        "  tree: |\n",
        "    +STRthat looks like a mapping key\n",
        "  fr:om: '@perlpunk'\n",
        "  tags: comment error mapping\n",
        "  fail: true\n",
        "  yaml: |\n",
        "    key: value\n",
        "    this is #not \n",
        "a: key\n",
        "  tree: |\n",
        "    +STR\n",
        "     +DOC\n",
        "      +MAP\n",
        "       =VAL :key\n",
        "       =VAL :value\n",
    )
    .as_bytes();

    let (&selector, payload) = INPUT.split_first().expect("fuzz input is not empty");
    assert_eq!(selector % 6, 0, "regression input changed fuzz branches");
    let payload = std::str::from_utf8(payload).expect("fuzz payload is UTF-8");

    let mut component = String::with_capacity(payload.len() * 2 + 1);
    component.push('x');
    for byte in payload.bytes() {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        component.push(char::from(HEX[usize::from(byte >> 4)]));
        component.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }

    let handle = format!("tag:example.com,2026:{component}");
    let yaml = format!("%YAML 1.2\n%TAG !e! {handle}\n---\nkey: !e!item {payload:?}\n");

    let mut options = Options::default();
    let limit = options.max_directive_bytes;
    let directive_bytes = b"TAG !e! ".len() + handle.len();
    assert!(directive_bytes > limit);
    assert_eq!(
        first_error(&yaml).kind(),
        &ErrorKind::DirectiveByteLimitExceeded { limit }
    );

    options.max_directive_bytes = directive_bytes;
    let events = Parser::new_from_str_with_options(&yaml, options)
        .map(|result| result.expect("generated YAML should parse with sufficient directive budget"))
        .collect::<Vec<_>>();
    let tagged = events
        .iter()
        .filter_map(|(event, _)| event.tag())
        .collect::<Vec<_>>();

    assert_eq!(tagged.len(), 1);
    assert_eq!(tagged[0].handle(), handle);
    assert_eq!(tagged[0].suffix(), "item");
    assert_eq!(tagged[0].original_handle(), "!e!");
}

#[test]
fn crlf_and_wide_character_spans() {
    assert_eq!(scalar_values("a: 1\r\nb: 2\r\n"), ["a", "1", "b", "2"]);
    assert_eq!(scalar_values("a: 1\rb: 2\r"), ["a", "1", "b", "2"]);
    assert_eq!(
        scalar_values_with_style("k: |\r\n  line1\r\n  line2\r\n", ScalarStyle::Literal),
        ["line1\nline2\n"]
    );
    assert_eq!(
        scalar_values_with_style("k: \"a\r\n  b\"\r\n", ScalarStyle::DoubleQuoted),
        ["a b"]
    );

    let yaml = "\u{5B57}: \u{503C}\n# \u{2605}\u{6CE8}\nb: 2\n";
    let interesting: Vec<_> = collect_ok_events(yaml)
        .into_iter()
        .filter_map(|(event, span)| match event {
            Event::Scalar(value, ..) => Some((
                value.into_owned(),
                span.byte_range(),
                span.slice(yaml).map(ToOwned::to_owned),
            )),
            #[cfg(feature = "comments")]
            Event::Comment(text, _) => Some((
                format!("#{text}"),
                span.byte_range(),
                span.slice(yaml).map(ToOwned::to_owned),
            )),
            _ => None,
        })
        .collect();

    assert_eq!(
        interesting,
        vec![
            (
                "\u{5B57}".to_string(),
                Some(0..3),
                Some("\u{5B57}".to_string())
            ),
            (
                "\u{503C}".to_string(),
                Some(5..8),
                Some("\u{503C}".to_string())
            ),
            (
                "# \u{2605}\u{6CE8}".to_string(),
                Some(9..17),
                Some("# \u{2605}\u{6CE8}".to_string())
            ),
            ("b".to_string(), Some(18..19), Some("b".to_string())),
            ("2".to_string(), Some(21..22), Some("2".to_string())),
        ]
        .into_iter()
        .filter(|(text, _, _)| cfg!(feature = "comments") || !text.starts_with('#'))
        .collect::<Vec<_>>()
    );
}

#[test]
fn explicit_key_indicator_at_end_of_input() {
    // `?` followed by end of input is an empty explicit key, the same as `?\n`.
    for input in ["?", "- ?", "clé:\r\n  ?", "key:\n- é\n?"] {
        let with_newline = format!("{input}\n");
        let expected: Vec<_> = collect_ok_events(&with_newline)
            .into_iter()
            .map(|(event, _)| event)
            .collect();
        for events in [
            Parser::new_from_str(input).collect::<Result<Vec<_>, _>>(),
            Parser::new_from_iter(input.chars()).collect::<Result<Vec<_>, _>>(),
            Parser::new_from_fallible_iter(input.chars().map(Ok::<_, ErrorKind>))
                .collect::<Result<Vec<_>, _>>(),
        ] {
            let events = events.unwrap_or_else(|error| panic!("{input:?}: {error}"));
            let empty_nodes: Vec<_> = events
                .iter()
                .filter(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == "~"))
                .collect();
            assert_eq!(empty_nodes.len(), 2, "empty key and value: {input:?}");
            for (_, span) in empty_nodes {
                assert!(span.is_empty(), "{input:?}: {span:?}");
                assert_eq!(span.start.index(), input.chars().count());
            }
            assert_eq!(
                events
                    .into_iter()
                    .map(|(event, _)| event)
                    .collect::<Vec<_>>(),
                expected,
                "{input:?}"
            );
        }
    }
}

#[test]
fn minimally_indented_quoted_continuations_preserve_folding_and_source_spans() {
    for (quote, style) in [
        ('\'', ScalarStyle::SingleQuoted),
        ('"', ScalarStyle::DoubleQuoted),
    ] {
        for newline in ["\n", "\r\n"] {
            for (prefix, indent) in [("clé: ", " "), ("outer:\n  clé: ", "   ")] {
                for (blank_line, expected) in [("", "un deux"), (newline, "un\ndeux")] {
                    let quoted = format!("{quote}un{newline}{blank_line}{indent}deux{quote}");
                    let input = format!("{prefix}{quoted}{newline}after: done{newline}");
                    let options = granit_parser::options! { strict_indentation: true };
                    let from_str = Parser::new_from_str_with_options(&input, options.clone())
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap_or_else(|error| panic!("{input:?}: {error}"));
                    for other in [
                        Parser::new_from_iter_with_options(input.chars(), options.clone())
                            .collect::<Result<Vec<_>, _>>(),
                        Parser::new_from_fallible_iter_with_options(
                            input.chars().map(Ok::<_, ErrorKind>),
                            options,
                        )
                        .collect::<Result<Vec<_>, _>>(),
                        Parser::new_from_str(&input).collect::<Result<Vec<_>, _>>(),
                    ] {
                        assert_eq!(other.unwrap(), from_str, "{input:?}");
                    }
                    let scalars: Vec<_> = from_str
                        .iter()
                        .filter_map(|(event, span)| {
                            if let Event::Scalar(value, actual_style, ..) = event {
                                Some((value.as_ref(), *actual_style, span))
                            } else {
                                None
                            }
                        })
                        .collect();
                    let (value, _, span) = scalars
                        .iter()
                        .find(|(_, actual_style, _)| *actual_style == style)
                        .expect("quoted value");
                    assert_eq!(*value, expected, "{input:?}");
                    assert_eq!(span.slice(&input), Some(quoted.as_str()));
                    assert_eq!(scalars.last().unwrap().0, "done");
                }
            }
        }
    }
}

#[test]
fn nel_and_double_bom_probes() {
    assert_eq!(scalar_values("a\u{85}b\n"), ["a\u{85}b"]);
    assert_eq!(scalar_values("\u{FEFF}\u{FEFF}a: b\n"), ["a", "b"]);
}
