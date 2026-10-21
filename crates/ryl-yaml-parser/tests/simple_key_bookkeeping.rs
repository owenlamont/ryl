mod support;
use granit_parser::{ErrorKind, Event, Marker, Options, Parser, ScanError, Span};

fn trace_all_inputs(source: &str, options: Options) -> Vec<Result<(Event<'_>, Span), ScanError>> {
    let mut from_str = Parser::new_from_str_with_options(source, options.clone());
    let trace = from_str.by_ref().collect::<Vec<_>>();
    assert!(from_str.next().is_none());
    assert!(from_str.next().is_none());
    let from_iter =
        Parser::new_from_iter_with_options(source.chars(), options.clone()).collect::<Vec<_>>();
    let from_fallible = Parser::new_from_fallible_iter_with_options(
        source.chars().map(Ok::<_, ErrorKind>),
        options,
    )
    .collect::<Vec<_>>();
    assert_eq!(trace, from_iter, "source: {source:?}");
    assert_eq!(trace, from_fallible, "source: {source:?}");
    trace
}

fn parse_all_inputs(source: &str, options: Options) -> Vec<(Event<'_>, Span)> {
    trace_all_inputs(source, options)
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap_or_else(|error| panic!("source: {source:?}: {error}"))
}

fn outline(trace: &[Result<(Event<'_>, Span), ScanError>]) -> Vec<String> {
    trace
        .iter()
        .map(|entry| match entry {
            Ok((Event::Scalar(value, ..), _)) => format!("scalar:{value}"),
            #[cfg(feature = "comments")]
            Ok((Event::Comment(value, ..), _)) => format!("comment:{value}"),
            Ok((Event::StreamStart, _)) => "+STR".into(),
            Ok((Event::StreamEnd, _)) => "-STR".into(),
            Ok((Event::DocumentStart(..), _)) => "+DOC".into(),
            Ok((Event::DocumentEnd, _)) => "-DOC".into(),
            Ok((Event::SequenceStart(..), _)) => "+SEQ".into(),
            Ok((Event::SequenceEnd, _)) => "-SEQ".into(),
            Ok((Event::MappingStart(..), _)) => "+MAP".into(),
            Ok((Event::MappingEnd, _)) => "-MAP".into(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn nested_collection_keys_preserve_inner_and_outer_mapping_order() {
    for (source, expected) in [
        (
            "[[{a: b}, c], d]: outer\nnext: value\n",
            "+STR +DOC +MAP +SEQ +SEQ +MAP scalar:a scalar:b -MAP scalar:c -SEQ scalar:d -SEQ scalar:outer scalar:next scalar:value -MAP -DOC -STR",
        ),
        (
            "[{[a, b]: inner}: outer, [c, d]: next]\n",
            "+STR +DOC +SEQ +MAP +MAP +SEQ scalar:a scalar:b -SEQ scalar:inner -MAP scalar:outer -MAP +MAP +SEQ scalar:c scalar:d -SEQ scalar:next -MAP -SEQ -DOC -STR",
        ),
    ] {
        let trace = trace_all_inputs(source, Options::default());
        assert_eq!(outline(&trace), expected.split_whitespace().collect::<Vec<_>>());
    }
}

#[test]
fn outer_key_remains_active_after_all_nested_candidates_are_removed() {
    // Exercise both inline and spilled simple-key storage. Each closing bracket removes an
    // inner candidate, but the outer collection still becomes a key at the final colon.
    for depth in [1, 2, 8, 9, 64, 255] {
        let source = format!("{}leaf{}: value\n", "[".repeat(depth), "]".repeat(depth));
        let events = parse_all_inputs(&source, Options::default());
        assert!(matches!(events[2].0, Event::MappingStart(..)));
        assert!(events[3..3 + depth]
            .iter()
            .all(|(event, _)| matches!(event, Event::SequenceStart(..))));
        assert!(matches!(&events[3 + depth].0, Event::Scalar(value, ..) if value == "leaf"));
        assert!(events[4 + depth..4 + 2 * depth]
            .iter()
            .all(|(event, _)| matches!(event, Event::SequenceEnd)));
        assert!(matches!(&events[4 + 2 * depth].0, Event::Scalar(value, ..) if value == "value"));
        assert_eq!(events[3].1.start, Marker::new(0, 1, 0));
        assert_eq!(events[3 + depth].1.byte_range(), Some(depth..depth + 4));
    }
}

#[test]
fn expiration_of_older_candidate_keeps_younger_collection_key_live() {
    for key in ["a", "é🦀"] {
        // At ':' the root '[' exceeds the limit by one character, while '[key] ' is
        // exactly at its limit and must still be resolved as an implicit mapping key.
        let source = format!("[[{key}] : value]\n");
        let options = granit_parser::options! {
            simple_key_max_lookahead: key.chars().count() + 3,
        };
        let trace = trace_all_inputs(&source, options);
        let expected =
            format!("+STR +DOC +SEQ +MAP +SEQ scalar:{key} -SEQ scalar:value -MAP -SEQ -DOC -STR");
        assert_eq!(
            outline(&trace),
            expected.split_whitespace().collect::<Vec<_>>()
        );
    }
}

#[test]
fn key_length_limit_is_strictly_greater_than_character_count_not_byte_count() {
    for key in ["abcd", "é字🦀x"] {
        let source = format!("a: b\n{key}: value\n");
        let options = granit_parser::options! { simple_key_max_lookahead: 4 };
        let events = parse_all_inputs(&source, options);
        let (_, span) = events
            .iter()
            .find(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == key))
            .expect("the key at the character limit is accepted");
        assert_eq!(span.start, Marker::new(5, 2, 0));
        assert_eq!(span.end, Marker::new(9, 2, 4));
        assert_eq!(span.byte_range(), Some(5..5 + key.len()));

        let options = granit_parser::options! { simple_key_max_lookahead: 3 };
        let trace = trace_all_inputs(&source, options);
        let error = trace.last().unwrap().as_ref().unwrap_err();
        assert_eq!(error.kind(), &ErrorKind::SimpleKeyExpected);
        assert_eq!(*error.marker(), Marker::new(5, 2, 0));
        assert_eq!(
            outline(&trace[..trace.len() - 1]),
            ["+STR", "+DOC", "+MAP", "scalar:a", "scalar:b"]
        );
    }
}

#[test]
fn zero_and_maximum_limits_preserve_explicit_keys_and_non_key_nodes() {
    for source in ["[a, [b], { ? c: d }]\n", "? a\n: b\n"] {
        let expected = parse_all_inputs(source, Options::default());
        for limit in [0, usize::MAX] {
            let options = granit_parser::options! { simple_key_max_lookahead: limit };
            assert_eq!(parse_all_inputs(source, options), expected);
        }
    }
    let source = format!("a: b\n{}: value\n", "é".repeat(2048));
    let options = granit_parser::options! { simple_key_max_lookahead: usize::MAX };
    parse_all_inputs(&source, options);
}

#[test]
fn missing_required_keys_keep_error_marker_and_preceding_event_timing() {
    for suffix in ["missing", "missing\n", "missing # hidden\n", "[[x], y]\n"] {
        let source = format!("a: b\n# ready\n{suffix}");
        let trace = trace_all_inputs(&source, Options::default());
        let error = trace.last().unwrap().as_ref().unwrap_err();
        assert_eq!(error.kind(), &ErrorKind::SimpleKeyExpected);
        assert_eq!(*error.marker(), Marker::new(13, 3, 0));
        assert_eq!(error.marker().byte_offset(), Some(13));
        // With parser comments enabled, the preceding comment is observable. Neither the
        // unresolved key nor a later comment may be published ahead of the required-key error.
        assert_eq!(
            outline(&trace[..trace.len() - 1]),
            [
                "+STR",
                "+DOC",
                "+MAP",
                "scalar:a",
                "scalar:b",
                "comment: ready"
            ]
            .into_iter()
            .filter(|event| cfg!(feature = "comments") || !event.starts_with("comment:"))
            .collect::<Vec<_>>()
        );
    }
}

#[test]
fn comments_and_document_markers_preserve_event_order() {
    let source = "# before\n---\n[[a], b]: value # after\n... # end\n--- # next\n{c: d}\n";
    let expected = [
        "+STR",
        "comment: before",
        "+DOC",
        "+MAP",
        "+SEQ",
        "+SEQ",
        "scalar:a",
        "-SEQ",
        "scalar:b",
        "-SEQ",
        "scalar:value",
        "comment: after",
        "-MAP",
        "-DOC",
        "comment: end",
        "+DOC",
        "comment: next",
        "+MAP",
        "scalar:c",
        "scalar:d",
        "-MAP",
        "-DOC",
        "-STR",
    ];
    for &emit_comments in support::comment_modes() {
        let options =
            granit_parser::options! { #[cfg(feature = "comments")] emit_comments: emit_comments };
        let trace = trace_all_inputs(source, options);
        assert_eq!(
            outline(&trace),
            expected
                .iter()
                .copied()
                .filter(|event| (cfg!(feature = "comments") && emit_comments)
                    || !event.starts_with("comment:"))
                .collect::<Vec<_>>()
        );
        #[cfg(feature = "comments")]
        for entry in &trace {
            if let Ok((Event::Comment(value, ..), span)) = entry {
                assert_eq!(span.slice(source), Some(format!("#{value}").as_str()));
            }
        }
    }
}

#[test]
fn under_indented_flow_entries_keep_supported_and_invalid_key_cases() {
    let source =
        "outer:\n  key: {\none:\n[first,\nsecond],\nthree: {four: five}\n}\n  after: done\n";
    let reference = "outer:\n  key: {one: [first, second], three: {four: five}}\n  after: done\n";
    let actual = parse_all_inputs(source, Options::default());
    let expected = parse_all_inputs(reference, Options::default());
    assert_eq!(
        actual.iter().map(|(event, _)| event).collect::<Vec<_>>(),
        expected.iter().map(|(event, _)| event).collect::<Vec<_>>()
    );

    let source = "key: { &anchor\nname\n: value\n}\n";
    let trace = trace_all_inputs(source, Options::default());
    let error = trace.last().unwrap().as_ref().unwrap_err();
    assert_eq!(error.kind(), &ErrorKind::InvalidColonPlacement);
    assert_eq!(*error.marker(), Marker::new(20, 3, 0));
}
