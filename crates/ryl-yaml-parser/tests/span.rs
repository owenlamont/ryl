#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::float_cmp)]
use ryl_yaml_parser::{ErrorKind, Event, Marker, Parser, ScanError};

fn char_index_to_byte_index(s: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    s.char_indices()
        .nth(char_index)
        .map_or_else(|| s.len(), |(byte, _)| byte)
}

fn span_offsets(input: &str, start: Marker, end: Marker) -> (usize, usize) {
    let start_b = start
        .byte_offset()
        .unwrap_or_else(|| char_index_to_byte_index(input, start.index()));
    let end_b = end
        .byte_offset()
        .unwrap_or_else(|| char_index_to_byte_index(input, end.index()));
    (start_b, end_b)
}

/// Run the parser through the string, returning all the scalars, and collecting their spans to strings.
fn run_parser_and_deref_scalar_spans(
    input: &str,
) -> Result<Vec<(String, String)>, ScanError> {
    let mut events = vec![];
    for x in Parser::new_from_str(input) {
        let x = x?;
        if let Event::Scalar(s, ..) = x.0 {
            let (start, end) = span_offsets(input, x.1.start, x.1.end);
            let input_s = &input[start..end];
            events.push((s.into(), input_s.to_string()));
        }
    }
    Ok(events)
}

/// Run the parser through the string, returning all the scalars, and collecting their spans to strings.
fn run_parser_and_deref_seq_spans(input: &str) -> Result<Vec<String>, ScanError> {
    let mut events = vec![];
    let mut start_stack = vec![];
    for x in Parser::new_from_str(input) {
        let x = x?;
        match x.0 {
            Event::SequenceStart(..) => start_stack.push(x.1.start),
            Event::SequenceEnd => {
                let start = start_stack.pop().unwrap();
                let (start, end) = span_offsets(input, start, x.1.end);
                let input_s = &input[start..end];
                events.push(input_s.to_string());
            }
            _ => {}
        }
    }
    Ok(events)
}

fn deref_pairs(pairs: &[(String, String)]) -> Vec<(&str, &str)> {
    pairs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect()
}

fn first_event_slice(
    yaml: &str,
    matches_event: impl Fn(&Event<'_>) -> bool,
) -> Option<String> {
    Parser::new_from_str(yaml)
        .filter_map(Result::ok)
        .find_map(|(event, span)| {
            matches_event(&event).then(|| span.slice(yaml).map(str::to_owned))?
        })
}

fn event_spans(yaml: &str) -> Vec<(Event<'_>, ryl_yaml_parser::Span)> {
    Parser::new_from_str(yaml).map(Result::unwrap).collect()
}

#[test]
fn span_helpers_report_length_empty_and_byte_range() {
    let span = ryl_yaml_parser::Span::new(
        Marker::new(2, 1, 2).with_byte_offset(Some(5)),
        Marker::new(6, 1, 6).with_byte_offset(Some(13)),
    );

    assert_eq!(span.len(), 4);
    assert!(!span.is_empty());
    assert_eq!(span.byte_range(), Some(5..13));
    assert_eq!(span.tag_start(), None);

    let empty =
        ryl_yaml_parser::Span::empty(Marker::new(6, 1, 6).with_byte_offset(Some(13)));
    assert!(empty.is_empty());
    assert_eq!(empty.byte_range(), Some(13..13));

    let without_byte_offsets =
        ryl_yaml_parser::Span::new(Marker::new(0, 1, 0), Marker::new(1, 1, 1));
    assert_eq!(without_byte_offsets.byte_range(), None);
}

#[test]
fn indentless_sequence_end_span_is_empty_before_the_next_key() {
    // The `?` that ends an indentless sequence belongs to the next mapping entry; the
    // SequenceEnd span is empty at its start, like other block collection ends.
    for (yaml, at) in [
        ("k:\n- a\n? y\n", 7),
        ("?\n- a\n?\n", 6),
        ("k:\n- a\nj: 1\n", 7),
    ] {
        let spans = event_spans(yaml);
        let end = spans
            .iter()
            .find(|(event, _)| matches!(event, Event::SequenceEnd))
            .map(|(_, span)| span.byte_range())
            .unwrap();
        assert_eq!(end, Some(at..at), "{yaml:?}");
        let mut last = 0;
        for (event, span) in &spans {
            let start = span.byte_range().unwrap().start;
            assert!(start >= last, "{yaml:?}: {event:?} goes backwards");
            last = start;
        }
    }
}

#[test]
fn indentless_sequence_end_spans_preserve_unicode_and_document_boundaries() {
    for (yaml, next_token, line, col) in [
        ("clé:\r\n- é\r\n? suite\r\n", "? suite", 3, 0),
        ("clé:\n- # empty\n? suite\n", "? suite", 3, 0),
        ("outer:\n  clé:\n  - é\nsuite: 1\n", "suite", 4, 0),
        ("clé:\n- é\n...\n", "...", 3, 0),
        ("clé:\n- é\n---\nsuivant\n", "---", 3, 0),
        ("clé:\n- é", "", 2, 3),
    ] {
        let offset = if next_token.is_empty() {
            yaml.len()
        } else {
            yaml.find(next_token).unwrap()
        };
        let marker = Marker::new(yaml[..offset].chars().count(), line, col);
        let from_str = event_spans(yaml);
        for other in [
            Parser::new_from_iter(yaml.chars()).collect::<Result<Vec<_>, _>>(),
            Parser::new_from_fallible_iter(yaml.chars().map(Ok::<_, ErrorKind>))
                .collect::<Result<Vec<_>, _>>(),
        ] {
            assert_eq!(other.unwrap(), from_str, "{yaml:?}");
        }
        let ends: Vec<_> = from_str
            .iter()
            .filter_map(|(event, span)| {
                matches!(event, Event::SequenceEnd).then_some(span)
            })
            .collect();
        assert_eq!(ends.len(), 1, "{yaml:?}");
        let span = ends[0];
        assert_eq!(span.start, marker, "{yaml:?}");
        assert_eq!(span.end, marker, "{yaml:?}");
        assert_eq!(span.byte_range(), Some(offset..offset), "{yaml:?}");
        assert_eq!(span.slice(yaml), Some(""));
        assert_eq!(span.indent, None);
        assert_eq!(span.tag_start(), None);
    }
}

#[test]
fn stream_and_block_collection_end_spans_use_physical_eof() {
    for (yaml, line, col, block_end_count) in [
        ("", 1, 0, 0),
        ("😀", 1, 1, 0),
        ("😀\n", 2, 0, 0),
        ("😀\r\n", 2, 0, 0),
        ("😀\r", 2, 0, 0),
        ("\r\n😀", 2, 1, 0),
        ("[]", 1, 2, 0),
        ("{}", 1, 2, 0),
        ("clé:", 1, 4, 1),
        ("-", 1, 1, 1),
        ("?", 1, 1, 1),
        ("outer:\n  - é", 2, 5, 2),
        ("outer:\r\n  - é\r\n", 3, 0, 2),
        ("clé:\n- é", 2, 3, 2),
    ] {
        let from_str = event_spans(yaml);
        for other in [
            Parser::new_from_iter(yaml.chars()).collect::<Result<Vec<_>, _>>(),
            Parser::new_from_fallible_iter(yaml.chars().map(Ok::<_, ErrorKind>))
                .collect::<Result<Vec<_>, _>>(),
        ] {
            assert_eq!(other.unwrap(), from_str, "{yaml:?}");
        }
        let marker = Marker::new(yaml.chars().count(), line, col);
        let (last, stream_end) =
            from_str.last().expect("stream must have an end event");
        assert_eq!(last, &Event::StreamEnd);

        // Flow endings cover their closing delimiter; block endings and StreamEnd
        // are empty spans at the actual end of the input, including its last line.
        let block_ends: Vec<_> = from_str
            .iter()
            .filter_map(|(event, span)| {
                (matches!(event, Event::SequenceEnd | Event::MappingEnd)
                    && span.is_empty())
                .then_some(span)
            })
            .collect();
        assert_eq!(block_ends.len(), block_end_count, "{yaml:?}");
        for span in block_ends.into_iter().chain([stream_end]) {
            assert_eq!(span.start, marker, "{yaml:?}");
            assert_eq!(span.end, marker, "{yaml:?}");
            assert_eq!(span.byte_range(), Some(yaml.len()..yaml.len()), "{yaml:?}");
            assert_eq!(span.slice(yaml), Some(""));
        }
    }
}

#[test]
fn flow_collection_event_spans_cover_only_the_indicators() {
    assert_eq!(
        first_event_slice("[ # c\n  a]\n", |event| matches!(
            event,
            Event::SequenceStart(..)
        ))
        .as_deref(),
        Some("[")
    );
    assert_eq!(
        first_event_slice("[a] # c\n", |event| matches!(event, Event::SequenceEnd))
            .as_deref(),
        Some("]")
    );
    assert_eq!(
        first_event_slice("{ # c\n  a: b}\n", |event| matches!(
            event,
            Event::MappingStart(..)
        ))
        .as_deref(),
        Some("{")
    );
    assert_eq!(
        first_event_slice("{a: b} # c\n", |event| matches!(event, Event::MappingEnd))
            .as_deref(),
        Some("}")
    );
}

#[test]
fn tagged_block_collection_reports_tag_start_on_tag_line() {
    let yaml = "key: !!omap # tag is here\n  a: 1\n";
    let (_event, span) = Parser::new_from_str(yaml)
        .map(Result::unwrap)
        .find(|(event, _span)| {
            matches!(
                event,
                Event::MappingStart(_, _, Some(tag)) if tag.suffix() == "omap"
            )
        })
        .expect("expected tagged block mapping");

    assert_eq!(span.start.line(), 2);

    let tag_start = span
        .tag_start()
        .expect("tagged node should report tag start");
    assert_eq!(tag_start.line(), 1);
    assert_eq!(tag_start.col(), 5);
    assert_eq!(tag_start.byte_offset(), Some(5));
}

#[test]
fn tag_start_uses_tag_token_when_anchor_precedes_tag() {
    let yaml = "key: &a !!str value\n";
    let (_event, span) = Parser::new_from_str(yaml)
        .map(Result::unwrap)
        .find(|(event, _span)| {
            matches!(
                event,
                Event::Scalar(value, _, _, Some(tag))
                    if value.as_ref() == "value" && tag.suffix() == "str"
            )
        })
        .expect("expected tagged anchored scalar");

    let tag_start = span
        .tag_start()
        .expect("tagged node should report tag start");
    assert_eq!(tag_start.line(), 1);
    assert_eq!(tag_start.col(), 8);
    assert_eq!(tag_start.byte_offset(), Some(8));
}

#[test]
fn tag_start_uses_tag_token_when_tag_precedes_anchor() {
    let yaml = "key: !!str &a value\n";
    let (_event, span) = Parser::new_from_str(yaml)
        .map(Result::unwrap)
        .find(|(event, _span)| {
            matches!(
                event,
                Event::Scalar(value, _, _, Some(tag))
                    if value.as_ref() == "value" && tag.suffix() == "str"
            )
        })
        .expect("expected tagged anchored scalar");

    let tag_start = span
        .tag_start()
        .expect("tagged node should report tag start");
    assert_eq!(tag_start.line(), 1);
    assert_eq!(tag_start.col(), 5);
    assert_eq!(tag_start.byte_offset(), Some(5));
}

#[test]
fn span_slice_returns_source_text_for_valid_byte_ranges() {
    let source = "key: value";
    let span = ryl_yaml_parser::Span::new(
        Marker::new(5, 1, 5).with_byte_offset(Some(5)),
        Marker::new(10, 1, 10).with_byte_offset(Some(10)),
    );

    assert_eq!(span.slice(source), Some("value"));
}

#[test]
fn span_slice_handles_non_ascii_byte_ranges() {
    let source = "a: 你好";
    let span = ryl_yaml_parser::Span::new(
        Marker::new(3, 1, 3).with_byte_offset(Some(3)),
        Marker::new(5, 1, 5).with_byte_offset(Some(source.len())),
    );
    let invalid_boundary = ryl_yaml_parser::Span::new(
        Marker::new(3, 1, 3).with_byte_offset(Some(4)),
        Marker::new(5, 1, 5).with_byte_offset(Some(source.len())),
    );

    assert_eq!(span.slice(source), Some("你好"));
    assert_eq!(invalid_boundary.slice(source), None);
}

#[test]
fn parser_spans_use_byte_offsets_for_non_ascii_input() {
    let source = "a: 你好\nb: c\n";
    let scalars: Vec<_> = Parser::new_from_str(source)
        .filter_map(|parsed| {
            let (event, span) = parsed.unwrap();
            if let Event::Scalar(value, ..) = event {
                Some((
                    value.into_owned(),
                    span.byte_range(),
                    span.slice(source).map(std::borrow::ToOwned::to_owned),
                ))
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        scalars,
        vec![
            ("a".to_string(), Some(0..1), Some("a".to_string())),
            ("你好".to_string(), Some(3..9), Some("你好".to_string())),
            ("b".to_string(), Some(10..11), Some("b".to_string())),
            ("c".to_string(), Some(13..14), Some("c".to_string())),
        ]
    );
}

#[test]
fn span_slice_handles_empty_spans() {
    let source = "key: value";
    let empty =
        ryl_yaml_parser::Span::empty(Marker::new(4, 1, 4).with_byte_offset(Some(4)));

    assert_eq!(empty.slice(source), Some(""));
}

#[test]
fn tagged_empty_scalar_span_stays_at_tag_end() {
    let yaml = "a: !!str\nb: c\n";
    let events = event_spans(yaml);
    let (_event, span) = events
        .iter()
        .find(|(event, _span)| {
            matches!(
                event,
                Event::Scalar(value, _, _, Some(tag))
                    if value.is_empty() && tag.suffix() == "str"
            )
        })
        .expect("expected tagged empty scalar");

    let tag_end = yaml.find('\n').unwrap();
    let next_key = yaml.find("b:").unwrap();

    assert_eq!(span.start.index(), tag_end);
    assert_eq!(span.end.index(), tag_end);
    assert_ne!(span.start.index(), next_key);
    assert_eq!(span.slice(yaml), Some(""));
}

#[test]
fn implicit_document_end_span_stays_at_previous_document_end() {
    let yaml = "foo\n--- bar\n";
    let events = event_spans(yaml);
    let (_event, span) = events
        .iter()
        .find(|(event, _span)| matches!(event, Event::DocumentEnd))
        .expect("expected implicit document end");

    let foo_end = yaml.find('\n').unwrap();
    let next_marker = yaml.find("---").unwrap();

    assert_eq!(span.start.index(), foo_end);
    assert_eq!(span.end.index(), foo_end);
    assert_ne!(span.start.index(), next_marker);
    assert_eq!(span.slice(yaml), Some(""));
}

#[test]
fn block_missing_value_span_is_empty_at_next_token_start() {
    let yaml = "? foo\nbar: baz\n";
    let events = event_spans(yaml);
    let empty_value_spans: Vec<_> = events
        .iter()
        .filter_map(|(event, span)| {
            matches!(event, Event::Scalar(value, ..) if value == "~").then_some(*span)
        })
        .collect();

    let next_key = yaml.find("bar").unwrap();

    assert_eq!(empty_value_spans.len(), 1);
    assert_eq!(empty_value_spans[0].start.index(), next_key);
    assert_eq!(empty_value_spans[0].end.index(), next_key);
    assert_eq!(empty_value_spans[0].slice(yaml), Some(""));
}

#[test]
fn span_slice_returns_none_for_buffered_input_spans_without_byte_offsets() {
    let source = "foo: bar";
    let mut scalar_slices = Vec::new();

    for parsed in Parser::new_from_iter(source.chars()) {
        let (event, span) = parsed.unwrap();
        if matches!(event, Event::Scalar(..)) {
            scalar_slices.push(span.slice(source));
        }
    }

    assert_eq!(scalar_slices, [None, None]);
}

#[test]
fn test_plain() {
    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans("foo: bar").unwrap()),
        [("foo", "foo"), ("bar", "bar"),]
    );
    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans("foo: bar ").unwrap()),
        [("foo", "foo"), ("bar", "bar"),]
    );
    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans("foo :  \t  bar\t ").unwrap()),
        [("foo", "foo"), ("bar", "bar"),]
    );

    assert_eq!(
        deref_pairs(
            &run_parser_and_deref_scalar_spans("foo :  \n  - bar\n  - baz\n ").unwrap()
        ),
        [("foo", "foo"), ("bar", "bar"), ("baz", "baz")]
    );
}

#[test]
fn test_plain_utf8() {
    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans("a: \u{4F60}\u{5273}").unwrap()),
        [("a", "a"), ("\u{4F60}\u{5273}", "\u{4F60}\u{5273}")]
    );
}

#[test]
fn test_quoted() {
    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans(r#"foo: "bar""#).unwrap()),
        [("foo", "foo"), ("bar", r#""bar""#),]
    );
    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans(r"foo: 'bar'").unwrap()),
        [("foo", "foo"), ("bar", r"'bar'"),]
    );

    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans(r#"foo: "bar ""#).unwrap()),
        [("foo", "foo"), ("bar ", r#""bar ""#),]
    );
}

#[test]
fn test_literal() {
    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans("foo: |\n  bar").unwrap()),
        [("foo", "foo"), ("bar\n", "bar"),]
    );
    assert_eq!(
        deref_pairs(
            &run_parser_and_deref_scalar_spans("foo: |\n  bar\n  more").unwrap()
        ),
        [("foo", "foo"), ("bar\nmore\n", "bar\n  more"),]
    );
}

#[test]
fn test_block() {
    assert_eq!(
        deref_pairs(&run_parser_and_deref_scalar_spans("foo: >\n  bar").unwrap()),
        [("foo", "foo"), ("bar\n", "bar"),]
    );
    assert_eq!(
        deref_pairs(
            &run_parser_and_deref_scalar_spans("foo: >\n  bar\n  more").unwrap()
        ),
        [("foo", "foo"), ("bar more\n", "bar\n  more"),]
    );
}

#[test]
fn test_seq() {
    assert_eq!(
        run_parser_and_deref_seq_spans("[a, b]").unwrap(),
        ["[a, b]"]
    );
    assert_eq!(
        run_parser_and_deref_seq_spans("- a\n- b").unwrap(),
        ["- a\n- b"]
    );
    assert_eq!(
        run_parser_and_deref_seq_spans("foo:\n  - a\n  - b").unwrap(),
        ["- a\n  - b"]
    );
    assert_eq!(
        run_parser_and_deref_seq_spans("foo:\n  - a\n  - bar:\n    - b\n    - c")
            .unwrap(),
        ["b\n    - c", "- a\n  - bar:\n    - b\n    - c"]
    );
}

#[test]
fn test_literal_utf8() {
    assert_eq!(
        deref_pairs(
            &run_parser_and_deref_scalar_spans("foo: |\n  \u{4F60}\u{5273}").unwrap()
        ),
        [("foo", "foo"), ("\u{4F60}\u{5273}\n", "\u{4F60}\u{5273}"),]
    );
    assert_eq!(
        deref_pairs(
            &run_parser_and_deref_scalar_spans(
                "foo: |\n  one:\u{4F60}\u{5273}\n  two:\u{4F60}\u{5273}"
            )
            .unwrap()
        ),
        [
            ("foo", "foo"),
            (
                "one:\u{4F60}\u{5273}\ntwo:\u{4F60}\u{5273}\n",
                "one:\u{4F60}\u{5273}\n  two:\u{4F60}\u{5273}"
            ),
        ]
    );
}

#[test]
fn test_block_utf8() {
    assert_eq!(
        deref_pairs(
            &run_parser_and_deref_scalar_spans("foo: >\n  \u{4F60}\u{5273}").unwrap()
        ),
        [("foo", "foo"), ("\u{4F60}\u{5273}\n", "\u{4F60}\u{5273}")],
    );
    assert_eq!(
        deref_pairs(
            &run_parser_and_deref_scalar_spans(
                "foo: >\n  one:\u{4F60}\u{5273}\n  two:\u{4F60}\u{5273}"
            )
            .unwrap()
        ),
        [
            ("foo", "foo"),
            (
                "one:\u{4F60}\u{5273} two:\u{4F60}\u{5273}\n",
                "one:\u{4F60}\u{5273}\n  two:\u{4F60}\u{5273}"
            )
        ],
    );
}

#[test]
fn span_slice_for_quoted_scalar_excludes_trailing_comment() {
    let yaml = "key: \"value\" # comment\n";
    let slices: Vec<_> = Parser::new_from_str(yaml)
        .filter_map(|parsed| {
            let (event, span) = parsed.unwrap();
            matches!(event, Event::Scalar(..))
                .then(|| span.slice(yaml).unwrap().to_string())
        })
        .collect();

    assert_eq!(slices, vec!["key".to_string(), "\"value\"".to_string()]);
}

#[test]
fn span_slice_for_single_quoted_scalar_excludes_trailing_comment() {
    let yaml = "key: 'value' # comment\n";
    let slices: Vec<_> = Parser::new_from_str(yaml)
        .filter_map(|parsed| {
            let (event, span) = parsed.unwrap();
            matches!(event, Event::Scalar(..))
                .then(|| span.slice(yaml).unwrap().to_string())
        })
        .collect();

    assert_eq!(slices, vec!["key".to_string(), "'value'".to_string()]);
}

#[test]
fn test_flow_sequence_explicit_mapping_end_span_order() {
    let input = "[? a: [b], ? c: &x d, ? e: !t f]";
    let mut last_end = 0usize;

    for parsed in Parser::new_from_str(input) {
        let (_event, span) = parsed.unwrap();
        let (_, end) = span_offsets(input, span.start, span.end);
        assert!(
            end >= last_end,
            "event end span regressed: current end {end} < previous end {last_end}"
        );
        last_end = end;
    }
}

#[test]
fn test_flow_sequence_explicit_empty_mapping_value_end_span_order() {
    let input = "[? a:, ? b: c]";
    let mut last_end = 0usize;

    for parsed in Parser::new_from_str(input) {
        let (_event, span) = parsed.unwrap();
        let (_, end) = span_offsets(input, span.start, span.end);
        assert!(
            end >= last_end,
            "event end span regressed: current end {end} < previous end {last_end}"
        );
        last_end = end;
    }
}
