mod support;
#[cfg(feature = "comments")]
use granit_parser::Placement;
use granit_parser::{Event, Parser, ScalarStyle, ScanError, Span};

/// Run the parser through the string.
///
/// The parser is run through both the `StrInput` and `BufferedInput` variants. The resulting
/// events are then compared and must match.
///
/// # Returns
/// This function returns the events and associated spans if parsing succeeds, the error the parser returned otherwise.
///
/// # Panics
/// This function panics if there is a mismatch between the 2 parser invocations with the different
/// input traits.
fn run_parser_with_span(input: &str) -> Result<Vec<(Event<'_>, Span)>, ScanError> {
    let mut str_events = vec![];
    let mut str_error = None;
    let mut iter_events = vec![];
    let mut iter_error = None;

    for x in Parser::new_from_str(input) {
        match x {
            Ok(event) => str_events.push(event),
            Err(e) => {
                str_error = Some(e);
                break;
            }
        }
    }
    for x in Parser::new_from_iter(input.chars()) {
        match x {
            Ok(event) => iter_events.push(event),
            Err(e) => {
                iter_error = Some(e);
                break;
            }
        }
    }

    assert_eq!(str_events, iter_events);
    assert_eq!(str_error, iter_error);

    if let Some(err) = str_error {
        Err(err)
    } else {
        Ok(str_events)
    }
}

#[test]
fn root_block_scalars_stop_at_document_markers() {
    for (indicator, style) in [('|', ScalarStyle::Literal), ('>', ScalarStyle::Folded)] {
        for (chomping, content, expected) in [
            ("", "text\n", "text\n"),
            ("", "", ""),
            ("-", "", ""),
            ("+", "", ""),
            ("", "text\n\n", "text\n"),
            ("-", "text\n\n", "text"),
            ("+", "text\n\n", "text\n\n"),
        ] {
            for (next_document, explicit, second) in [
                ("---\nsecond", true, "second"),
                ("--- second\n", true, "second"),
                ("---\tsecond\n", true, "second"),
                ("---\r\nsecond\r\n", true, "second"),
                ("---", true, "~"),
                ("...\nsecond\n", false, "second"),
            ] {
                let yaml = format!("{indicator}{chomping}\n{content}{next_document}");
                let spanned_events = run_parser_with_span(&yaml).unwrap();
                let marker_start = yaml.len() - next_document.len();
                assert_eq!(
                    spanned_events[2].1.end.index(),
                    marker_start,
                    "input: {yaml:?}"
                );
                if explicit {
                    assert_eq!(spanned_events[4].1.start.index(), marker_start);
                    assert_eq!(spanned_events[4].1.slice(&yaml), Some("---"));
                }
                let events: Vec<_> = spanned_events.into_iter().map(|(event, _)| event).collect();
                assert_eq!(
                    events,
                    [
                        Event::StreamStart,
                        Event::DocumentStart(false, None),
                        Event::Scalar(expected.into(), style, 0, None),
                        Event::DocumentEnd,
                        Event::DocumentStart(explicit, None),
                        Event::Scalar(second.into(), ScalarStyle::Plain, 0, None),
                        Event::DocumentEnd,
                        Event::StreamEnd,
                    ],
                    "input: {yaml:?}",
                );
            }
        }
    }
}

#[test]
fn root_block_scalars_preserve_folding_and_unicode_positions_before_document_markers() {
    for (indicator, style, body) in [
        ('|', ScalarStyle::Literal, "\nα\nβ"),
        ('>', ScalarStyle::Folded, "\nα β"),
    ] {
        for (chomping, tail) in [("-", ""), ("", "\n"), ("+", "\n\n")] {
            for newline in ["\n", "\r\n", "\r"] {
                let prefix =
                    format!("{indicator}{chomping}{newline}{newline}α{newline}β{newline}{newline}");
                let yaml = format!("{prefix}--- # next document{newline}second{newline}");
                let events = run_parser_with_span(&yaml).unwrap();

                assert_eq!(
                    events
                        .iter()
                        .map(|(event, _)| event.clone())
                        .collect::<Vec<_>>(),
                    [
                        Event::StreamStart,
                        Event::DocumentStart(false, None),
                        Event::Scalar(format!("{body}{tail}").into(), style, 0, None),
                        Event::DocumentEnd,
                        Event::DocumentStart(true, None),
                        #[cfg(feature = "comments")]
                        Event::Comment(" next document".into(), Placement::Right),
                        Event::Scalar("second".into(), ScalarStyle::Plain, 0, None),
                        Event::DocumentEnd,
                        Event::StreamEnd,
                    ]
                    .into_iter()
                    .filter(|event| cfg!(feature = "comments") || !support::is_comment_event(event))
                    .collect::<Vec<_>>(),
                    "input: {yaml:?}",
                );

                let scalar_span = events[2].1;
                let document_span = events[4].1;
                assert_eq!(scalar_span.indent, Some(0));
                assert_eq!(scalar_span.end.index(), prefix.chars().count());
                assert_eq!(scalar_span.end.byte_offset(), Some(prefix.len()));
                assert_eq!(scalar_span.end.line(), 6);
                assert_eq!(scalar_span.end.col(), 0);
                assert_eq!(document_span.start, scalar_span.end);
                assert_eq!(
                    document_span.byte_range(),
                    Some(prefix.len()..prefix.len() + 3)
                );
                assert_eq!(document_span.slice(&yaml), Some("---"));
            }
        }
    }
}

#[test]
fn block_scalars_preserve_content_resembling_document_markers() {
    for (indicator, style) in [('|', ScalarStyle::Literal), ('>', ScalarStyle::Folded)] {
        for (content, expected) in [
            ("---text\n", "---text\n"),
            ("---#text\n", "---#text\n"),
            ("----\n", "----\n"),
            ("...text\n", "...text\n"),
            ("....\n", "....\n"),
            ("  ---\n", "---\n"),
            ("  ...\n", "...\n"),
            ("text\n  ---\n", "text\n  ---\n"),
            ("text\n  ...\n", "text\n  ...\n"),
        ] {
            let yaml = format!("{indicator}\n{content}");
            let events: Vec<_> = run_parser_with_span(&yaml)
                .unwrap()
                .into_iter()
                .map(|(event, _)| event)
                .collect();
            assert_eq!(
                events,
                [
                    Event::StreamStart,
                    Event::DocumentStart(false, None),
                    Event::Scalar(expected.into(), style, 0, None),
                    Event::DocumentEnd,
                    Event::StreamEnd,
                ],
                "input: {yaml:?}",
            );
        }
    }
}

#[test]
fn test_document_end_emitted_immediately() {
    // Test that DocumentEnd event is emitted immediately after the document end marker (...)
    // without reading more content ahead.
    // The span of DocumentEnd should end right after the "..." marker.
    let s = "foo\n...\nbar";
    //       0123 456 789...
    //       foo\n = 0-3 (4 chars)
    //       ... = 4-6 (3 chars)
    //       \n = 7
    //       bar = 8-10

    let events = run_parser_with_span(s).unwrap();

    // Find the DocumentEnd event and check its span
    let doc_end_event = events
        .iter()
        .find(|(ev, _)| matches!(ev, Event::DocumentEnd))
        .expect("DocumentEnd event should exist");

    // The DocumentEnd span should start at position 4 (start of "...")
    // and end at position 7 (right after "...")
    assert_eq!(
        doc_end_event.1.start.index(),
        4,
        "DocumentEnd should start at the '...' marker"
    );
    assert_eq!(
        doc_end_event.1.end.index(),
        7,
        "DocumentEnd should end right after the '...' marker"
    );
}

#[test]
fn test_document_start_emitted_immediately() {
    // Test that DocumentStart event is emitted immediately after the document start marker (---)
    // without reading more content ahead.
    let s = "---\nfoo";
    //       0123 456
    //       --- = 0-2 (3 chars)
    //       \n = 3
    //       foo = 4-6

    let events = run_parser_with_span(s).unwrap();

    // Find the DocumentStart event and check its span
    let doc_start_event = events
        .iter()
        .find(|(ev, _)| matches!(ev, Event::DocumentStart(true, None)))
        .expect("DocumentStart(true) event should exist");

    // The DocumentStart span should start at position 0 (start of "---")
    // and end at position 3 (right after "---")
    assert_eq!(
        doc_start_event.1.start.index(),
        0,
        "DocumentStart should start at the '---' marker"
    );
    assert_eq!(
        doc_start_event.1.end.index(),
        3,
        "DocumentStart should end right after the '---' marker"
    );
}

#[test]
fn test_document_end_emitted_immediately_on_next_document_start_marker() {
    // Test that DocumentEnd event is emitted immediately when the parser encounters a new
    // document start marker ("---") at the beginning of a line.
    //
    // In YAML, a `---` marker can implicitly terminate the previous document.
    // The synthetic `DocumentEnd` should be emitted before the subsequent
    // `DocumentStart(true)`, but its span belongs to the end of the previous document, not to the
    // following `---` marker.
    let s = "foo\n---\nbar";
    //       0123 456 789...
    //       foo\n = 0-3 (4 chars)
    //       --- = 4-6 (3 chars)
    //       \n = 7
    //       bar = 8-10

    let events = run_parser_with_span(s).unwrap();

    // Find the index of the DocumentEnd event.
    let doc_end_idx = events
        .iter()
        .position(|(ev, _)| matches!(ev, Event::DocumentEnd))
        .expect("DocumentEnd event should exist");

    // The next event must be DocumentStart(true) for the second document.
    let (next_ev, next_span) = events
        .get(doc_end_idx + 1)
        .expect("DocumentStart(true) should follow DocumentEnd");
    assert!(
        matches!(next_ev, Event::DocumentStart(true, None)),
        "DocumentStart(true) should immediately follow DocumentEnd"
    );

    // DocumentEnd should be a zero-width span at the end of the previous document.
    let doc_end_span = events[doc_end_idx].1;
    assert_eq!(
        doc_end_span.start.index(),
        3,
        "DocumentEnd should start at the end of the previous document"
    );
    assert_eq!(
        doc_end_span.end.index(),
        3,
        "DocumentEnd should end at the end of the previous document"
    );

    // Sanity: the DocumentStart marker span should match.
    assert_eq!(
        next_span.start.index(),
        4,
        "DocumentStart should start at the '---' marker"
    );
    assert_eq!(
        next_span.end.index(),
        7,
        "DocumentStart should end right after the '---' marker"
    );
}
