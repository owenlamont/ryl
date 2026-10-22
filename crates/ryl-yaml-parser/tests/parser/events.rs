use std::borrow::Cow;
use std::fmt;

use ryl_yaml_parser::{
    ErrorKind, Event, EventReceiver, Marker, ParseResult, Parser, ParserTrait,
    ScalarStyle, ScanError, Span, StructureStyle, Tag, TryEventReceiver, TryLoadError,
    TrySpannedEventReceiver, YamlVersion,
};

#[derive(Default)]
struct CollectingSink<'input> {
    events: Vec<Event<'input>>,
}

impl<'input> EventReceiver<'input> for CollectingSink<'input> {
    fn on_event(&mut self, ev: Event<'input>) {
        self.events.push(ev);
    }
}

fn first_error_info(input: &str) -> String {
    for event in Parser::new_from_str(input) {
        if let Err(err) = event {
            return err.info();
        }
    }
    panic!("expected parser error")
}

fn first_tagged_scalar_tag(input: &str) -> Tag {
    Parser::new_from_str(input)
        .find_map(|event| match event.expect("input should parse").0 {
            Event::Scalar(_, _, _, Some(tag)) => Some(tag.into_owned()),
            _ => None,
        })
        .expect("expected tagged scalar")
}

#[test]
fn display_resolved_core_tag_without_extra_bang() {
    let tag = Tag::with_original_handle("tag:yaml.org,2002:", "str", "!!");

    assert_eq!(tag.to_string(), "tag:yaml.org,2002:str");
}

#[test]
fn tag_helpers_distinguish_core_and_local_tags() {
    let core = Tag::with_original_handle("tag:yaml.org,2002:", "int", "!!");
    let local = Tag::new("!", "thing");
    let non_specific = Tag::with_original_handle("", "!", "");
    let verbatim = Tag::with_original_handle("", "tag:example.com,2000:thing", "");
    let unknown_yaml_org =
        Tag::with_original_handle("", "tag:yaml.org,2002:application", "");

    assert_eq!(core.core_suffix(), Some("int"));
    assert!(core.is_yaml_core_schema());
    assert!(core.is_yaml_core_schema_tag("int"));
    assert!(!core.is_yaml_core_schema_tag("str"));
    assert!(!core.is_custom());
    assert_eq!(core.parts(), ("tag:yaml.org,2002:", "int"));
    assert_eq!(core.original_parts(), ("!!", "int"));
    assert_eq!(core.original(), "!!int");

    assert_eq!(local.core_suffix(), None);
    assert!(!local.is_yaml_core_schema());
    assert!(!local.is_yaml_core_schema_tag("thing"));
    assert!(local.is_custom());
    assert_eq!(local.parts(), ("!", "thing"));
    assert_eq!(local.original_parts(), ("!", "thing"));
    assert_eq!(local.original(), "!thing");
    assert_eq!(local.to_string(), "!thing");

    assert_eq!(non_specific.parts(), ("", "!"));
    assert_eq!(non_specific.original_parts(), ("", "!"));
    assert_eq!(non_specific.original(), "!");

    assert_eq!(verbatim.parts(), ("", "tag:example.com,2000:thing"));
    assert_eq!(
        verbatim.original_parts(),
        ("", "tag:example.com,2000:thing")
    );
    assert_eq!(verbatim.original(), "!<tag:example.com,2000:thing>");

    assert_eq!(unknown_yaml_org.core_suffix(), None);
    assert!(!unknown_yaml_org.is_yaml_core_schema());
    assert!(unknown_yaml_org.is_custom());
}

#[test]
fn core_suffix_uses_resolved_tag_uri_for_common_spellings() {
    let cases = [
        ("shorthand", "v: !!int 1\n", ("tag:yaml.org,2002:", "int")),
        (
            "verbatim",
            "v: !<tag:yaml.org,2002:int> 1\n",
            ("", "tag:yaml.org,2002:int"),
        ),
        (
            "full prefix",
            "%TAG !e! tag:yaml.org,2002:\n---\nv: !e!int 1\n",
            ("tag:yaml.org,2002:", "int"),
        ),
        (
            "mid-split",
            "%TAG !m! tag:yaml.org,2002:i\n---\nv: !m!nt 1\n",
            ("tag:yaml.org,2002:i", "nt"),
        ),
    ];

    for (label, input, expected_parts) in cases {
        let tag = first_tagged_scalar_tag(input);

        assert_eq!(tag.parts(), expected_parts, "{label}");
        assert_eq!(tag.core_suffix(), Some("int"), "{label}");
        assert!(tag.is_yaml_core_schema(), "{label}");
        assert!(tag.is_yaml_core_schema_tag("int"), "{label}");
        assert!(!tag.is_yaml_core_schema_tag("str"), "{label}");
        assert!(!tag.is_custom(), "{label}");
    }
}

#[test]
fn core_suffix_rejects_non_core_yaml_org_tags() {
    let cases = [
        "binary",
        "merge",
        "omap",
        "pairs",
        "set",
        "timestamp",
        "value",
        "yaml",
    ];

    for suffix in cases {
        let tag = Tag::with_original_handle("tag:yaml.org,2002:", suffix, "!!");

        assert_eq!(tag.core_suffix(), None, "{suffix}");
        assert!(!tag.is_yaml_core_schema(), "{suffix}");
        assert!(tag.is_custom(), "{suffix}");
    }
}

#[test]
fn core_suffix_rejects_non_core_tags() {
    let cases = [
        ("local", "v: !local 1\n"),
        ("verbatim custom", "v: !<tag:example.com,2000:int> 1\n"),
        (
            "custom directive",
            "%TAG !e! tag:example.com,2000:\n---\nv: !e!int 1\n",
        ),
        (
            "overridden secondary handle",
            "%TAG !! tag:example.com,2000:app/\n---\nv: !!int 1\n",
        ),
    ];

    for (label, input) in cases {
        let tag = first_tagged_scalar_tag(input);

        assert_eq!(tag.core_suffix(), None, "{label}");
        assert!(!tag.is_yaml_core_schema(), "{label}");
        assert!(!tag.is_yaml_core_schema_tag("int"), "{label}");
        assert!(tag.is_custom(), "{label}");
    }
}

#[test]
fn suffix_in_namespace_resolves_across_spellings() {
    const NS: &str = "tag:yaml.org,2002:";

    // Every spelling of `tag:yaml.org,2002:omap` resolves to the same name, even though
    // `omap` is not a Core Schema type (so `core_suffix` reports `None`). `mid_split` cuts
    // the URI after the namespace prefix (handle longer than `NS`); `inside_split` cuts it
    // before the prefix ends (handle a non-empty prefix of `NS`), exercising both branches.
    let shorthand = Tag::with_original_handle(NS, "omap", "!!");
    let verbatim = Tag::with_original_handle("", "tag:yaml.org,2002:omap", "");
    let mid_split = Tag::with_original_handle("tag:yaml.org,2002:o", "map", "!o!");
    let inside_split = Tag::with_original_handle("tag:yaml.org,", "2002:omap", "!y!");
    for tag in [&shorthand, &verbatim, &mid_split, &inside_split] {
        assert_eq!(tag.suffix_in_namespace(NS).as_deref(), Some("omap"));
        assert_eq!(tag.core_suffix(), None);
    }

    // Borrow whenever the resolved name is a contiguous slice of `handle` or `suffix`;
    // allocate only when a split lands inside the name itself (handle extends past `NS`).
    assert!(matches!(
        shorthand.suffix_in_namespace(NS),
        Some(Cow::Borrowed(_))
    ));
    assert!(matches!(
        verbatim.suffix_in_namespace(NS),
        Some(Cow::Borrowed(_))
    ));
    assert!(matches!(
        inside_split.suffix_in_namespace(NS),
        Some(Cow::Borrowed(_))
    ));
    assert!(matches!(
        mid_split.suffix_in_namespace(NS),
        Some(Cow::Owned(_))
    ));

    // The non-core `merge` type resolves the same way; core types still flow through
    // `core_suffix`.
    let merge = Tag::with_original_handle(NS, "merge", "!!");
    assert_eq!(merge.suffix_in_namespace(NS).as_deref(), Some("merge"));
    assert_eq!(merge.core_suffix(), None);
    assert_eq!(
        Tag::new(NS, "int").suffix_in_namespace(NS).as_deref(),
        Some("int")
    );

    // Tags outside the namespace do not resolve into it.
    assert_eq!(Tag::new("!", "omap").suffix_in_namespace(NS), None);
    assert_eq!(
        Tag::with_original_handle("", "tag:example.com,2000:omap", "")
            .suffix_in_namespace(NS),
        None
    );
}

#[test]
fn event_inspection_helpers_report_node_metadata() {
    let tag = Tag::new("!", "thing");
    let scalar = Event::Scalar(
        "value".into(),
        ScalarStyle::DoubleQuoted,
        7,
        Some(Cow::Borrowed(&tag)),
    );
    let sequence =
        Event::SequenceStart(StructureStyle::Block, 8, Some(Cow::Owned(tag.clone())));
    let mapping =
        Event::MappingStart(StructureStyle::Block, 9, Some(Cow::Borrowed(&tag)));

    assert_eq!(scalar.anchor_id(), Some(7));
    assert_eq!(scalar.alias_id(), None);
    assert_eq!(scalar.tag(), Some(&tag));
    assert_eq!(scalar.scalar(), Some(("value", ScalarStyle::DoubleQuoted)));
    assert!(scalar.is_node());

    assert_eq!(sequence.anchor_id(), Some(8));
    assert_eq!(sequence.alias_id(), None);
    assert_eq!(sequence.tag(), Some(&tag));
    assert_eq!(sequence.scalar(), None);
    assert!(sequence.is_node());

    assert_eq!(mapping.anchor_id(), Some(9));
    assert_eq!(mapping.alias_id(), None);
    assert_eq!(mapping.tag(), Some(&tag));
    assert_eq!(mapping.scalar(), None);
    assert!(mapping.is_node());

    let alias = Event::Alias(10);
    assert_eq!(alias.anchor_id(), None);
    assert_eq!(alias.alias_id(), Some(10));
    assert_eq!(alias.tag(), None);
    assert_eq!(alias.scalar(), None);
    assert!(alias.is_node());

    let unanchored_scalar = Event::Scalar("x".into(), ScalarStyle::Plain, 0, None);
    assert_eq!(unanchored_scalar.anchor_id(), None);
    assert_eq!(unanchored_scalar.alias_id(), None);

    let stream_start = Event::StreamStart;
    assert_eq!(stream_start.anchor_id(), None);
    assert_eq!(stream_start.alias_id(), None);
    assert_eq!(stream_start.tag(), None);
    assert_eq!(stream_start.scalar(), None);
    assert!(!stream_start.is_node());
}

#[test]
fn test_peek_eq_parse() {
    let s = "
a0 bb: val
a1: &x
b1: 4
b2: d
a2: 4
a3: [1, 2, 3]
a4:
- [a1, a2]
- 2
a5: *x
";
    let mut p = Parser::new_from_str(s);
    loop {
        let event_peek = p.peek().unwrap().unwrap().clone();
        let event = p.next_event().unwrap().unwrap();
        assert_eq!(event, event_peek);
        if event.0 == Event::StreamEnd {
            break;
        }
    }
}

#[test]
fn test_repeated_peek_returns_buffered_event() {
    let mut parser = Parser::new_from_str("key: value\n");

    let first_peek = parser.peek().unwrap().unwrap().clone();
    let second_peek = parser.peek().unwrap().unwrap().clone();
    let next = parser.next_event().unwrap().unwrap();

    assert_eq!(first_peek, second_peek);
    assert_eq!(first_peek, next);
}

#[test]
fn test_peek_surfaces_scan_error_without_consuming_stream_end_state() {
    let mut parser = Parser::new_from_str("a: [1, 2");

    loop {
        match parser.peek() {
            Some(Ok(_)) => {
                parser.next_event().unwrap().unwrap();
            }
            Some(Err(error)) => {
                assert_eq!(error.info(), "unclosed bracket '['");
                break;
            }
            None => panic!("expected parse error"),
        }
    }
}

#[test]
fn test_iterator_terminates_after_scan_error() {
    let parser = Parser::new_from_str("foo:\n  bar\ninvalid\n");
    let mut errors = 0usize;
    let mut events = 0usize;

    for item in parser {
        events += 1;
        if item.is_err() {
            errors += 1;
        }
        assert!(
            events < 1000,
            "parser iterator did not terminate after a scan error"
        );
    }

    assert_eq!(errors, 1);
}

#[test]
fn test_iterator_terminates_after_node_property_error() {
    let parser = Parser::new_from_str("- *nope\n- 2\n");
    let mut errors = 0usize;
    let mut saw_later_node = false;
    let mut events = 0usize;

    for item in parser {
        events += 1;
        match item {
            Ok((Event::Scalar(value, ..), _)) if value == "2" => saw_later_node = true,
            Ok(_) => {}
            Err(error) => {
                assert_eq!(error.info(), "while parsing node, found unknown anchor");
                errors += 1;
            }
        }
        assert!(
            events < 1000,
            "parser iterator did not terminate after a node-property error"
        );
    }

    assert_eq!(errors, 1);
    assert!(!saw_later_node, "parser resumed after the alias error");
}

#[test]
fn test_peeked_scan_error_is_returned_once_by_next_event() {
    let mut parser = Parser::new_from_str("a: [1, 2");

    let first_error = loop {
        match parser.peek() {
            Some(Ok(_)) => {
                parser.next_event().unwrap().unwrap();
            }
            Some(Err(error)) => break error,
            None => panic!("expected parse error"),
        }
    };
    let Some(Err(second_error)) = parser.peek() else {
        panic!("expected cached parse error");
    };

    assert_eq!(first_error, second_error);
    assert_eq!(parser.next_event().unwrap().unwrap_err(), first_error);
    assert!(parser.next_event().is_none());
    assert!(parser.peek().is_none());
}

#[test]
fn test_peeked_node_property_error_is_stable_and_terminal() {
    let mut parser = Parser::new_from_str("a: *nope\nb: 2\n");

    for _ in 0..4 {
        parser.next_event().unwrap().unwrap();
    }

    let Some(Err(first_error)) = parser.peek() else {
        panic!("expected unknown alias error");
    };
    let Some(Err(second_error)) = parser.peek() else {
        panic!("expected cached unknown alias error");
    };

    assert_eq!(first_error, second_error);
    assert_eq!(
        first_error.info(),
        "while parsing node, found unknown anchor"
    );
    assert_eq!(parser.next_event().unwrap().unwrap_err(), first_error);
    assert!(parser.next_event().is_none());
    assert!(parser.peek().is_none());
}

#[test]
fn test_peek_and_next_return_none_after_stream_end() {
    let mut parser = Parser::new_from_str("");

    assert!(matches!(
        parser.next_event().unwrap().unwrap().0,
        Event::StreamStart
    ));
    assert!(matches!(
        parser.next_event().unwrap().unwrap().0,
        Event::StreamEnd
    ));
    assert!(parser.next_event().is_none());
    assert!(parser.peek().is_none());
}

#[test]
fn test_load_after_stream_already_ended_emits_stream_end() {
    let mut parser = Parser::new_from_str("");
    while parser.next_event().is_some() {}

    let mut sink = CollectingSink::default();
    parser.load(&mut sink, true).unwrap();

    assert_eq!(sink.events, vec![Event::StreamEnd]);
}

#[test]
fn test_load_full_stream_fuses_iterator_after_stream_end() {
    let mut parser = Parser::new_from_str("a: 1\n");
    let mut sink = CollectingSink::default();

    parser.load(&mut sink, true).unwrap();

    assert!(matches!(sink.events.last(), Some(Event::StreamEnd)));
    assert!(parser.next_event().is_none());
    assert!(parser.peek().is_none());
}

#[test]
fn test_load_after_peek_delivers_buffered_document_end_before_stream_end() {
    let mut parser = Parser::new_from_str("a");
    for _ in 0..3 {
        parser.next_event().unwrap().unwrap();
    }

    assert_eq!(parser.peek().unwrap().unwrap().0, Event::DocumentEnd);

    let mut sink = CollectingSink::default();
    parser.load(&mut sink, true).unwrap();

    assert_eq!(sink.events, vec![Event::DocumentEnd, Event::StreamEnd]);
    assert!(parser.next_event().is_none());
}

#[test]
fn test_load_visits_nested_collection_events() {
    let mut parser = Parser::new_from_str("root:\n  - item: value\n  - [a, b]\n");
    let mut sink = CollectingSink::default();

    parser.load(&mut sink, true).unwrap();

    assert_eq!(
        sink.events,
        vec![
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar("root".into(), ScalarStyle::Plain, 0, None),
            Event::SequenceStart(StructureStyle::Block, 0, None),
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar("item".into(), ScalarStyle::Plain, 0, None),
            Event::Scalar("value".into(), ScalarStyle::Plain, 0, None),
            Event::MappingEnd,
            Event::SequenceStart(StructureStyle::Flow, 0, None),
            Event::Scalar("a".into(), ScalarStyle::Plain, 0, None),
            Event::Scalar("b".into(), ScalarStyle::Plain, 0, None),
            Event::SequenceEnd,
            Event::SequenceEnd,
            Event::MappingEnd,
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ValidationError {
    ForbiddenValue,
}

struct FailingSink<'input> {
    events: Vec<Event<'input>>,
}

impl<'input> TryEventReceiver<'input> for FailingSink<'input> {
    type Error = ValidationError;

    fn on_event(&mut self, ev: Event<'input>) -> Result<(), Self::Error> {
        let should_fail =
            matches!(&ev, Event::Scalar(value, ..) if value.as_ref() == "bad");
        self.events.push(ev);
        if should_fail {
            Err(ValidationError::ForbiddenValue)
        } else {
            Ok(())
        }
    }
}

#[test]
fn test_try_load_stops_on_receiver_error() {
    let mut parser = Parser::new_from_str("ok: bad\nafter: value\n");
    let mut sink = FailingSink { events: Vec::new() };

    let err = parser.try_load(&mut sink, true).unwrap_err();

    assert_eq!(err, TryLoadError::Receiver(ValidationError::ForbiddenValue));
    assert!(
        sink.events
            .iter()
            .any(|event| matches!(event, Event::Scalar(value, ..) if value == "ok"))
    );
    assert!(
        sink.events
            .iter()
            .any(|event| matches!(event, Event::Scalar(value, ..) if value == "bad"))
    );
    assert!(
        !sink
            .events
            .iter()
            .any(|event| matches!(event, Event::Scalar(value, ..) if value == "after"))
    );
}

struct SpannedFailingSink {
    failed_span: Option<Span>,
}

impl<'input> TrySpannedEventReceiver<'input> for SpannedFailingSink {
    type Error = Span;

    fn on_event(&mut self, ev: Event<'input>, span: Span) -> Result<(), Self::Error> {
        if matches!(ev, Event::Scalar(value, ..) if value.as_ref() == "bad") {
            self.failed_span = Some(span);
            Err(span)
        } else {
            Ok(())
        }
    }
}

#[test]
fn test_try_load_spanned_receiver_gets_span() {
    let mut parser = Parser::new_from_str("value: bad\n");
    let mut sink = SpannedFailingSink { failed_span: None };

    let err = parser.try_load(&mut sink, false).unwrap_err();

    let TryLoadError::Receiver(span) = err else {
        panic!("expected receiver error");
    };

    assert_eq!(Some(span), sink.failed_span);
    assert!(!span.is_empty());
}

struct NeverFails {
    count: usize,
}

impl<'input> TryEventReceiver<'input> for NeverFails {
    type Error = ValidationError;

    fn on_event(&mut self, _ev: Event<'input>) -> Result<(), Self::Error> {
        self.count += 1;
        Ok(())
    }
}

#[test]
fn test_try_load_returns_scan_error() {
    let mut parser = Parser::new_from_str("%YAML 1.2\n%YAML 1.2\n---\n");
    let mut sink = NeverFails { count: 0 };

    let err = parser.try_load(&mut sink, true).unwrap_err();

    let TryLoadError::Scan(err) = err else {
        panic!("expected scan error");
    };
    assert_eq!(err.kind(), &ErrorKind::DuplicateVersionDirective);
    assert_eq!(err.info(), "duplicate version directive");
}

#[test]
fn test_try_load_after_stream_already_ended_emits_stream_end() {
    let mut parser = Parser::new_from_str("");
    while parser.next_event().is_some() {}

    let mut sink = FailingSink { events: Vec::new() };
    parser.try_load(&mut sink, true).unwrap();

    assert_eq!(sink.events, vec![Event::StreamEnd]);
}

#[test]
fn test_try_load_full_stream_fuses_iterator_after_stream_end() {
    let mut parser = Parser::new_from_str("a: 1\n");
    let mut sink = FailingSink { events: Vec::new() };

    parser.try_load(&mut sink, true).unwrap();

    assert!(matches!(sink.events.last(), Some(Event::StreamEnd)));
    assert!(parser.next_event().is_none());
    assert!(parser.peek().is_none());
}

#[test]
fn test_try_load_after_peek_delivers_buffered_document_end_before_stream_end() {
    let mut parser = Parser::new_from_str("a");
    for _ in 0..3 {
        parser.next_event().unwrap().unwrap();
    }

    assert_eq!(parser.peek().unwrap().unwrap().0, Event::DocumentEnd);

    let mut sink = FailingSink { events: Vec::new() };
    parser.try_load(&mut sink, true).unwrap();

    assert_eq!(sink.events, vec![Event::DocumentEnd, Event::StreamEnd]);
    assert!(parser.next_event().is_none());
}

#[test]
fn test_load_single_document_stops_before_next_document() {
    let mut parser = Parser::new_from_str("a: 1\n---\nb: 2\n");
    let mut sink = CollectingSink::default();

    parser.load(&mut sink, false).unwrap();

    assert!(
        sink.events
            .iter()
            .any(|event| matches!(event, Event::Scalar(value, ..) if value == "a"))
    );
    assert!(
        !sink
            .events
            .iter()
            .any(|event| matches!(event, Event::Scalar(value, ..) if value == "b"))
    );
    assert!(matches!(sink.events.last(), Some(Event::DocumentEnd)));
}

#[test]
fn test_duplicate_version_directive_errors() {
    assert_eq!(
        first_error_info("%YAML 1.2\n%YAML 1.2\n---\n"),
        "duplicate version directive"
    );
}

#[test]
fn test_unsupported_yaml_major_version_errors() {
    assert_eq!(
        first_error_info("%YAML 9.9\n--- a\n"),
        "unsupported YAML major version"
    );
}

#[test]
fn test_document_start_emits_yaml_version() {
    let events = Parser::new_from_str("%YAML 1.2\n---\nvalue\n")
        .map(|event| event.unwrap().0)
        .collect::<Vec<_>>();

    assert!(matches!(
        events.get(1),
        Some(Event::DocumentStart(
            true,
            Some(YamlVersion { major: 1, minor: 2 })
        ))
    ));
}

#[test]
fn test_document_start_allows_supported_major_future_minor_version() {
    let events = Parser::new_from_str("%YAML 1.9\n---\nvalue\n")
        .map(|event| event.unwrap().0)
        .collect::<Vec<_>>();

    assert!(matches!(
        events.get(1),
        Some(Event::DocumentStart(
            true,
            Some(YamlVersion { major: 1, minor: 9 })
        ))
    ));
}

#[test]
fn test_document_start_keeps_version_and_tags_across_comment() {
    let events = Parser::new_from_str(
        "%YAML 1.2\n# directive comment\n%TAG !e! tag:example.com,2026:\n---\nkey: !e!thing value\n",
    )
    .map(|event| event.unwrap().0)
    .filter(|event| {
        {
            !matches!(event, Event::Comment(..))
        }
    })
    .collect::<Vec<_>>();

    assert!(matches!(
        events.get(1),
        Some(Event::DocumentStart(
            true,
            Some(YamlVersion { major: 1, minor: 2 })
        ))
    ));

    let tag = events
        .iter()
        .find_map(|event| match event {
            Event::Scalar(value, _, _, Some(tag)) if value == "value" => Some(tag),
            _ => None,
        })
        .expect("expected tagged scalar after comment-separated directives");

    assert_eq!(tag.handle(), "tag:example.com,2026:");
    assert_eq!(tag.suffix(), "thing");
}

#[test]
fn test_each_document_can_declare_own_yaml_version() {
    let document_starts = Parser::new_from_str(
        "%YAML 1.2\n---\na\n...\n%YAML 1.2\n---\nb\n...\n%YAML 1.1\n---\nc\n",
    )
    .filter_map(|event| match event.unwrap().0 {
        Event::DocumentStart(explicit, version) => Some((explicit, version)),
        _ => None,
    })
    .collect::<Vec<_>>();

    assert_eq!(
        document_starts,
        vec![
            (true, Some(YamlVersion::new(1, 2))),
            (true, Some(YamlVersion::new(1, 2))),
            (true, Some(YamlVersion::new(1, 1))),
        ]
    );
}

#[test]
fn test_duplicate_tag_directive_errors() {
    assert_eq!(
        first_error_info("%TAG !t! tag:test,2024:\n%TAG !t! tag:other,2024:\n---\n"),
        "the TAG directive must only be given at most once per handle in the same document"
    );
}

#[test]
fn duplicate_tag_directive_across_comment_is_rejected() {
    let input = concat!(
        "%TAG !e! tag:example.com,2000:one/\n",
        "# separator\n",
        "%TAG !e! tag:example.com,2000:two/\n",
        "---\n",
    );

    assert_eq!(
        first_error_info(input),
        "the TAG directive must only be given at most once per handle in the same document"
    );
}

#[test]
fn test_directive_after_implicit_document_requires_explicit_end() {
    assert_eq!(
        first_error_info("---\nkey: value\n%YAML 1.2\n---\n"),
        "missing explicit document end marker before directive"
    );
}

#[test]
fn test_anchor_offset_overflow_reports_error() {
    let mut parser = Parser::new_from_str("&a value");
    parser.set_anchor_offset(usize::MAX);

    let err = parser
        .find_map(Result::err)
        .expect("anchor registration should overflow");

    assert_eq!(
        err.info(),
        "while parsing anchor, anchor count exceeded supported limit"
    );
}

#[test]
fn test_alias_resolves_to_registered_anchor_id() {
    let events = Parser::new_from_str("- &a value\n- *a\n")
        .map(|event| event.unwrap().0)
        .collect::<Vec<_>>();

    assert!(events.iter().any(|event| matches!(event, Event::Alias(1))));
}

#[test]
fn test_anchor_then_tag_applies_both_to_scalar() {
    let events = Parser::new_from_str("&a !!str value")
        .map(|event| event.unwrap().0)
        .collect::<Vec<_>>();

    let Some(Event::Scalar(value, _, anchor_id, Some(tag))) = events
        .iter()
        .find(|event| matches!(event, Event::Scalar(value, ..) if value == "value"))
    else {
        panic!("expected tagged anchored scalar");
    };

    assert_eq!(value, "value");
    assert_eq!(*anchor_id, 1);
    assert_eq!(tag.handle(), "tag:yaml.org,2002:");
    assert_eq!(tag.suffix(), "str");
    assert_eq!(tag.original_handle(), "!!");
    assert_eq!(tag.original(), "!!str");
}

#[test]
fn test_tag_then_anchor_applies_both_to_scalar() {
    let events = Parser::new_from_str("!!str &a value")
        .map(|event| event.unwrap().0)
        .collect::<Vec<_>>();

    let Some(Event::Scalar(value, _, anchor_id, Some(tag))) = events
        .iter()
        .find(|event| matches!(event, Event::Scalar(value, ..) if value == "value"))
    else {
        panic!("expected tagged anchored scalar");
    };

    assert_eq!(value, "value");
    assert_eq!(*anchor_id, 1);
    assert_eq!(tag.handle(), "tag:yaml.org,2002:");
    assert_eq!(tag.suffix(), "str");
    assert_eq!(tag.original_handle(), "!!");
    assert_eq!(tag.original(), "!!str");
}

#[test]
fn test_tag_directive_preserves_original_handle() {
    let events = Parser::new_from_str(
        "%TAG !e! tag:example.com,2000:\n---\nconfig: !e!keep value\n",
    )
    .map(|event| event.unwrap().0)
    .collect::<Vec<_>>();

    let (value, tag) = events
        .iter()
        .find_map(|event| match event {
            Event::Scalar(value, _, _, Some(tag)) if value == "value" => {
                Some((value, tag))
            }
            _ => None,
        })
        .expect("expected tagged scalar");

    assert_eq!(value, "value");
    assert_eq!(tag.handle(), "tag:example.com,2000:");
    assert_eq!(tag.suffix(), "keep");
    assert_eq!(tag.original_handle(), "!e!");
    assert_eq!(tag.parts(), ("tag:example.com,2000:", "keep"));
    assert_eq!(tag.original_parts(), ("!e!", "keep"));
    assert_eq!(tag.original(), "!e!keep");
}

#[test]
fn test_verbatim_tag_original_is_normalized_author_spelling() {
    let events = Parser::new_from_str("key: !<tag:example.com,2000:thing> value\n")
        .map(|event| event.unwrap().0)
        .collect::<Vec<_>>();

    let Some(Event::Scalar(value, _, _, Some(tag))) = events
        .iter()
        .find(|event| matches!(event, Event::Scalar(value, ..) if value == "value"))
    else {
        panic!("expected tagged scalar");
    };

    assert_eq!(value, "value");
    assert_eq!(tag.handle(), "");
    assert_eq!(tag.suffix(), "tag:example.com,2000:thing");
    assert_eq!(tag.original_handle(), "");
    assert_eq!(tag.parts(), ("", "tag:example.com,2000:thing"));
    assert_eq!(tag.original_parts(), ("", "tag:example.com,2000:thing"));
    assert_eq!(tag.original(), "!<tag:example.com,2000:thing>");
}

#[test]
fn test_multiple_tag_directives_are_kept_within_document() {
    let text = r"
%TAG !a! tag:a,2024:
%TAG !b! tag:b,2024:
---
first: !a!x foo
second: !b!y bar
";

    let mut seen_a = false;
    let mut seen_b = false;
    for event in Parser::new_from_str(text) {
        let (event, _) = event.unwrap();
        if let Event::Scalar(_, _, _, Some(tag)) = event {
            if tag.handle() == "tag:a,2024:" {
                seen_a = true;
            } else if tag.handle() == "tag:b,2024:" {
                seen_b = true;
            }
        }
    }

    assert!(seen_a);
    assert!(seen_b);
}

#[test]
fn test_tags_are_cleared_when_next_document_has_no_directives() {
    let text = r"
%TAG !t! tag:test,2024:
--- !t!1
foo
--- !t!2
bar
";

    let mut parser = Parser::new_from_str(text);
    for event in parser.by_ref() {
        let (event, _) = event.unwrap();
        if let Event::DocumentEnd = event {
            break;
        }
    }

    match parser.next().unwrap().unwrap().0 {
        Event::DocumentStart(true, None) => {}
        _ => panic!("expected explicit second document start"),
    }

    let err = parser.next().unwrap().unwrap_err();
    assert!(format!("{err}").contains("the handle wasn't declared"));
}

#[test]
fn test_pull_parser_clears_anchors_between_documents() {
    let mut parser = Parser::new_from_str(
        "--- &a value
--- *a
",
    );

    for event in parser.by_ref() {
        let (event, _) = event.unwrap();
        if matches!(event, Event::DocumentEnd) {
            break;
        }
    }

    match parser.next().unwrap().unwrap().0 {
        Event::DocumentStart(true, None) => {}
        _ => panic!("expected explicit second document start"),
    }

    let err = parser.next().unwrap().unwrap_err();
    assert!(format!("{err}").contains("unknown anchor"));
}

#[test]
fn test_keep_tags_across_multiple_documents() {
    let text = r#"
%YAML 1.1
%TAG !t! tag:test,2024:
--- !t!1 &1
foo: "bar"
--- !t!2 &2
baz: "qux"
"#;
    for x in Parser::new_from_str(text).keep_tags(true) {
        let x = x.unwrap();
        if let Event::MappingStart(_, _, tag) = x.0 {
            let tag = tag.unwrap();
            assert_eq!(tag.handle(), "tag:test,2024:");
        }
    }

    for x in Parser::new_from_str(text).keep_tags(false) {
        if x.is_err() {
            // Test successful
            return;
        }
    }
    panic!("Test failed, did not encounter error")
}

#[test]
fn test_flow_sequence_mapping_allows_empty_key() {
    let parser = Parser::new_from_str("[?: value]");
    for event in parser {
        event.expect("parser should accept flow sequence mappings with empty keys");
    }
}

#[test]
fn test_load_after_peek_stream_start() {
    #[derive(Default)]
    struct Sink<'input> {
        events: Vec<Event<'input>>,
    }

    impl<'input> EventReceiver<'input> for Sink<'input> {
        fn on_event(&mut self, ev: Event<'input>) {
            self.events.push(ev);
        }
    }

    let mut parser = Parser::new_from_str("key: value\n");
    let mut sink = Sink::default();

    assert_eq!(parser.peek().unwrap().unwrap().0, Event::StreamStart);
    parser.load(&mut sink, false).unwrap();

    assert!(matches!(sink.events.first(), Some(Event::StreamStart)));
    assert!(matches!(sink.events.get(1), Some(Event::DocumentStart(..))));
}

#[derive(Debug, PartialEq)]
struct Rejected;

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("rejected")
    }
}

impl core::error::Error for Rejected {}

/// Rejects the first event `reject` selects.
struct RejectWhen(fn(&Event<'_>) -> bool);

impl<'input> TryEventReceiver<'input> for RejectWhen {
    type Error = Rejected;

    fn on_event(&mut self, ev: Event<'input>) -> Result<(), Self::Error> {
        if (self.0)(&ev) { Err(Rejected) } else { Ok(()) }
    }
}

/// A caller-implemented parser replaying `self.0` from the back.
struct FailingSource(Vec<ParseResult<'static>>);

impl ParserTrait<'static> for FailingSource {
    fn peek(&mut self) -> Option<Result<&(Event<'static>, Span), ScanError>> {
        unimplemented!("the default load methods only read events")
    }

    fn next_event(&mut self) -> Option<ParseResult<'static>> {
        self.0.pop()
    }

    fn load<R: ryl_yaml_parser::SpannedEventReceiver<'static>>(
        &mut self,
        _recv: &mut R,
        _multi: bool,
    ) -> Result<(), ScanError> {
        unimplemented!("only the default try_load is under test")
    }
}

#[test]
fn try_load_errors_render_and_chain_their_cause() {
    use core::error::Error as _;

    let scan =
        TryLoadError::<Rejected>::Scan(ScanError::new(Marker::new(0, 1, 0), "boom"));
    assert_eq!(
        scan.to_string(),
        "parser error: boom at char 0 line 1 column 1"
    );
    assert_eq!(
        scan.source().unwrap().to_string(),
        "boom at char 0 line 1 column 1"
    );
    let receiver = TryLoadError::<Rejected>::Receiver(Rejected);
    assert_eq!(receiver.to_string(), "receiver error: rejected");
    assert_eq!(receiver.source().unwrap().to_string(), "rejected");
}

#[test]
fn try_load_reports_a_source_failure_before_the_stream_starts() {
    let failure = ErrorKind::InputDecoding {
        message: "bad byte".to_owned(),
    };
    let mut parser =
        Parser::new_from_fallible_iter(std::iter::once(Err(failure.clone())));
    let error = parser
        .try_load(&mut RejectWhen(|_| false), true)
        .unwrap_err();
    assert!(matches!(error, TryLoadError::Scan(error) if *error.kind() == failure));
}

#[test]
fn try_load_stops_at_a_rejected_stream_start_or_end() {
    for input in ["", "a"] {
        let mut parser = Parser::new_from_str(input);
        let error = parser
            .try_load(&mut RejectWhen(|ev| *ev == Event::StreamStart), true)
            .unwrap_err();
        assert_eq!(error, TryLoadError::Receiver(Rejected), "{input:?}");
        let mut parser = Parser::new_from_str(input);
        let error = parser
            .try_load(&mut RejectWhen(|ev| *ev == Event::StreamEnd), true)
            .unwrap_err();
        assert_eq!(error, TryLoadError::Receiver(Rejected), "{input:?}");
    }
}

#[test]
fn default_try_load_propagates_a_caller_parser_error() {
    let error = ScanError::new(Marker::new(0, 1, 0), "caller failure");
    let mut source = FailingSource(vec![
        Err(error.clone()),
        Ok((Event::StreamStart, Span::empty(Marker::new(0, 1, 0)))),
    ]);
    assert_eq!(
        source.try_load(&mut RejectWhen(|_| false), true),
        Err(TryLoadError::Scan(error))
    );
}

#[test]
fn try_load_after_the_stream_was_read_still_reports_stream_end() {
    let mut parser = Parser::new_from_str("a");
    while parser.next_event().is_some() {}
    let error = parser
        .try_load(&mut RejectWhen(|ev| *ev == Event::StreamEnd), true)
        .unwrap_err();
    assert_eq!(error, TryLoadError::Receiver(Rejected));
}
