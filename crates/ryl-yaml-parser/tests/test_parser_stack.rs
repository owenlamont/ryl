#[cfg(feature = "comments")]
use granit_parser::Placement;
extern crate alloc;

use alloc::{
    borrow::Cow,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::iter::Empty;
use granit_parser::{
    BorrowedInput, ErrorKind, Event, Marker, Parser, ParserStack, ParserTrait, ReplayParser,
    ScalarStyle, ScanError, Span, SpannedEventReceiver, StrInput, StructureStyle, TryEventReceiver,
    TryLoadError,
};

type MyStack<'a> = ParserStack<'a, Empty<char>, StrInput<'a>>;

fn test_span() -> Span {
    Span::empty(Marker::new(0, 1, 0))
}

fn plain_scalar(value: &'static str, anchor_id: usize) -> Event<'static> {
    Event::Scalar(value.into(), ScalarStyle::Plain, anchor_id, None)
}

fn collect_events<'a, I, T>(stack: &mut ParserStack<'a, I, T>) -> Result<Vec<Event<'a>>, String>
where
    I: Iterator<Item = char>,
    T: BorrowedInput<'a>,
{
    let mut events = Vec::new();
    loop {
        match stack.next_event() {
            Some(Ok((ev, _))) => {
                let is_end = matches!(ev, Event::StreamEnd);
                events.push(ev);
                if is_end {
                    break;
                }
            }
            Some(Err(e)) => return Err(e.to_string()),
            None => break,
        }
    }
    Ok(events)
}

fn find_anchor_id(events: &[Event], value: &str) -> Option<usize> {
    events.iter().find_map(|event| {
        if let Event::Scalar(scalar, _, anchor_id, _) = event {
            (scalar.as_ref() == value).then_some(*anchor_id)
        } else {
            None
        }
    })
}

fn stack_after_first_parent_anchor() -> MyStack<'static> {
    let mut stack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("before: &parent parent-value\nafter: &after resumed-parent-value\n"),
        "parent.yaml".to_string(),
    );

    loop {
        let event = stack.next_event().unwrap().unwrap().0;
        if let Event::Scalar(value, _, anchor_id, _) = event {
            if value == "parent-value" {
                assert_eq!(anchor_id, 1);
                break;
            }
        }
    }

    assert_eq!(stack.current_anchor_offset(), 2);
    stack
}

fn assert_include_anchor_offset_is_inherited(events: &[Event]) {
    assert_eq!(find_anchor_id(events, "included-value"), Some(2));
    assert!(events.iter().any(|event| matches!(event, Event::Alias(2))));
    assert_eq!(find_anchor_id(events, "resumed-parent-value"), Some(3));
}

fn format_events(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            Event::StreamStart => "StreamStart".to_string(),
            Event::StreamEnd => "StreamEnd".to_string(),
            Event::DocumentStart(..) => "DocStart".to_string(),
            Event::DocumentEnd => "DocEnd".to_string(),
            #[cfg(feature = "comments")]
            Event::Comment(text, _) => alloc::format!("Comment({})", text.as_ref()),
            Event::Scalar(val, _, _, _) => alloc::format!("Scalar({})", val.as_ref()),
            Event::MappingStart(..) => "MapStart".to_string(),
            Event::MappingEnd => "MapEnd".to_string(),
            Event::SequenceStart(..) => "SeqStart".to_string(),
            Event::SequenceEnd => "SeqEnd".to_string(),
            _ => "Other".to_string(),
        })
        .collect()
}

#[test]
fn test_single_parser() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: b"), "p1".to_string());

    let events = collect_events(&mut stack).unwrap();
    let names = format_events(&events);

    assert_eq!(
        names,
        vec![
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(a)",
            "Scalar(b)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn test_two_parsers_switching() {
    let mut stack: MyStack = ParserStack::new();
    // pushed first, so it's at the bottom (yields last)
    stack.push_str_parser(Parser::new_from_str("a: 1"), "p1".to_string());
    // pushed second, so it's at the top (yields first)
    stack.push_str_parser(Parser::new_from_str("b: 2"), "p2".to_string());

    let events = collect_events(&mut stack).unwrap();
    let names = format_events(&events);

    assert_eq!(
        names,
        vec![
            "MapStart",
            "Scalar(b)",
            "Scalar(2)",
            "MapEnd",
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(a)",
            "Scalar(1)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn test_two_parsers_second_has_two_docs_error() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: 1"), "p1".to_string());
    // p2 is top. it has two documents. this should fail.
    stack.push_str_parser(Parser::new_from_str("b: 2\n---\nc: 3"), "p2".to_string());

    let res = collect_events(&mut stack);
    assert!(res.is_err());
    assert!(res
        .unwrap_err()
        .contains("multiple documents not supported here"));
}

#[test]
fn nested_parser_scan_error_after_document_end_preserves_kind_and_adds_source_stack() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("parent: value"), "parent".to_string());
    stack.push_str_parser(
        Parser::new_from_str("child: value\n...\n[\n"),
        "child".to_string(),
    );

    loop {
        match stack.next_event() {
            Some(Ok(_)) => {}
            Some(Err(err)) => {
                assert_eq!(err.kind(), &ErrorKind::UnclosedFlowCollection { open: '[' });
                assert_eq!(
                    err.info(),
                    "unclosed bracket '['\nwhile parsing parent -> child"
                );
                assert_eq!(err.source_stack(), ["parent", "child"]);
                assert_eq!(stack.stack(), vec!["parent".to_string()]);
                break;
            }
            None => panic!("expected nested scan error"),
        }
    }
}

#[test]
fn test_two_parsers_first_has_multiple_docs_fine() {
    let mut stack: MyStack = ParserStack::new();
    // p1 is bottom. It can have multiple documents.
    stack.push_str_parser(Parser::new_from_str("a: 1\n---\nc: 3"), "p1".to_string());
    // p2 is top. Single document.
    stack.push_str_parser(Parser::new_from_str("b: 2"), "p2".to_string());

    let events = collect_events(&mut stack).unwrap();
    let names = format_events(&events);

    assert_eq!(
        names,
        vec![
            // p2
            "MapStart",
            "Scalar(b)",
            "Scalar(2)",
            "MapEnd",
            // p1 doc 1
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(a)",
            "Scalar(1)",
            "MapEnd",
            "DocEnd",
            // p1 doc 2
            "DocStart",
            "MapStart",
            "Scalar(c)",
            "Scalar(3)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn test_three_parsers_dynamic_adding() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("p1: 1"), "p1".to_string());

    // Fetch first event from top parser (p1)
    let ev1 = stack.next_event().unwrap().unwrap().0;
    assert!(matches!(ev1, Event::StreamStart));

    // Now push middle parser
    stack.push_str_parser(Parser::new_from_str("p2: 2"), "p2".to_string());

    // Fetch first event from middle parser (p2)
    let ev2 = stack.next_event().unwrap().unwrap().0;
    assert!(matches!(ev2, Event::MappingStart(..)));

    // Now push third parser
    stack.push_str_parser(Parser::new_from_str("p3: 3"), "p3".to_string());

    // Consume the rest
    let events = collect_events(&mut stack).unwrap();
    let names = format_events(&events);

    // p3 content:
    let mut expected = vec!["MapStart", "Scalar(p3)", "Scalar(3)", "MapEnd"];
    // p2 rest (already yielded MapStart, so now it's Scalar(p2)):
    expected.extend(vec!["Scalar(p2)", "Scalar(2)", "MapEnd"]);
    // p1 rest (already yielded StreamStart):
    expected.extend(vec![
        "DocStart",
        "MapStart",
        "Scalar(p1)",
        "Scalar(1)",
        "MapEnd",
        "DocEnd",
        "StreamEnd",
    ]);

    let expected_names: Vec<String> = expected
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    assert_eq!(names, expected_names);
}

#[test]
fn test_anchor_id_propagation() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("k1: &a v1\nk3: &c v3"),
        "p1".to_string(),
    );

    let mut events = Vec::new();

    // Read until v1 is consumed
    loop {
        let ev = stack.next_event().unwrap().unwrap().0;
        let is_v1 = if let Event::Scalar(val, _, anchor_id, _) = &ev {
            if val.as_ref() == "v1" {
                assert_eq!(*anchor_id, 1, "First anchor should have ID 1");
                true
            } else {
                false
            }
        } else {
            false
        };

        events.push(ev);
        if is_v1 {
            break;
        }
    }

    // Push inner parser after consuming first anchor event
    stack.push_str_parser(Parser::new_from_str("k2: &b v2"), "p2".to_string());

    // Consume the rest
    loop {
        match stack.next_event() {
            Some(Ok((ev, _))) => {
                let is_end = matches!(ev, Event::StreamEnd);
                events.push(ev);
                if is_end {
                    break;
                }
            }
            Some(Err(e)) => panic!("Parse error: {e}"),
            None => break,
        }
    }

    // Verify anchor IDs for v2 and v3
    let v2_ev = events
        .iter()
        .find(|e| matches!(e, Event::Scalar(v, _, _, _) if v.as_ref() == "v2"))
        .unwrap();
    if let Event::Scalar(_, _, id, _) = v2_ev {
        assert_eq!(*id, 2, "Second anchor (from inner parser) should have ID 2");
    }

    let v3_ev = events
        .iter()
        .find(|e| matches!(e, Event::Scalar(v, _, _, _) if v.as_ref() == "v3"))
        .unwrap();
    if let Event::Scalar(_, _, id, _) = v3_ev {
        assert_eq!(
            *id, 3,
            "Third anchor (from parent parser after inner) should have ID 3"
        );
    }
}

struct TestReceiver<'input> {
    events: Vec<Event<'input>>,
}

impl<'input> SpannedEventReceiver<'input> for TestReceiver<'input> {
    fn on_event(&mut self, ev: Event<'input>, _span: Span) {
        self.events.push(ev);
    }
}

struct TryTestReceiver<'input> {
    events: Vec<Event<'input>>,
}

impl<'input> TryEventReceiver<'input> for TryTestReceiver<'input> {
    type Error = &'static str;

    fn on_event(&mut self, ev: Event<'input>) -> Result<(), Self::Error> {
        let should_fail = matches!(&ev, Event::Scalar(value, ..) if value.as_ref() == "stop");
        self.events.push(ev);
        if should_fail {
            Err("stop requested")
        } else {
            Ok(())
        }
    }
}

#[test]
fn test_parser_stack_load() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: 1\n---\nc: 3"), "p1".to_string());
    stack.push_str_parser(Parser::new_from_str("b: 2"), "p2".to_string());

    let mut recv = TestReceiver { events: Vec::new() };

    // Load with multi = true
    stack.load(&mut recv, true).unwrap();

    let names = format_events(&recv.events);

    assert_eq!(
        names,
        vec![
            // p2
            "MapStart",
            "Scalar(b)",
            "Scalar(2)",
            "MapEnd",
            // p1 doc 1
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(a)",
            "Scalar(1)",
            "MapEnd",
            "DocEnd",
            // p1 doc 2
            "DocStart",
            "MapStart",
            "Scalar(c)",
            "Scalar(3)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn test_parser_stack_try_load_stops_on_receiver_error() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("a: stop\nafter: value\n"),
        "p1".to_string(),
    );

    let mut recv = TryTestReceiver { events: Vec::new() };

    let err = stack.try_load(&mut recv, true).unwrap_err();

    assert_eq!(err, TryLoadError::Receiver("stop requested"));
    assert!(recv
        .events
        .iter()
        .any(|event| matches!(event, Event::Scalar(value, ..) if value == "stop")));
    assert!(!recv
        .events
        .iter()
        .any(|event| matches!(event, Event::Scalar(value, ..) if value == "after")));
}

#[test]
fn test_parser_stack_load_single() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: 1\n---\nc: 3"), "p1".to_string());
    stack.push_str_parser(Parser::new_from_str("b: 2"), "p2".to_string());

    let mut recv = TestReceiver { events: Vec::new() };

    // Load with multi = false
    stack.load(&mut recv, false).unwrap();

    let names = format_events(&recv.events);

    assert_eq!(
        names,
        vec![
            // Nested parsers are inlined as subtree events, so their DocumentEnd is suppressed.
            // With multi = false, loading stops at the first DocumentEnd emitted by the parent parser.
            "MapStart",
            "Scalar(b)",
            "Scalar(2)",
            "MapEnd",
            // p1 doc 1
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(a)",
            "Scalar(1)",
            "MapEnd",
            "DocEnd"
        ]
    );
}

#[test]
fn test_iterator_impl() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: b"), "p1".to_string());

    let mut events = Vec::new();
    for ev in stack {
        let (e, _) = ev.unwrap();
        events.push(e);
    }

    let names = format_events(&events);
    assert_eq!(
        names,
        vec![
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(a)",
            "Scalar(b)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn test_include_resolver() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: 1"), "p1".to_string());

    stack.set_resolver(|name| {
        if name == "inc1" {
            Ok("b: 2".to_string())
        } else {
            unreachable!("this test only resolves inc1")
        }
    });

    stack.push_include("inc1").unwrap();

    let events = collect_events(&mut stack).unwrap();
    let names = format_events(&events);

    assert_eq!(
        names,
        vec![
            "MapStart",
            "Scalar(b)",
            "Scalar(2)",
            "MapEnd",
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(a)",
            "Scalar(1)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn borrowed_include_resolver_preserves_borrowed_event_text() {
    const INCLUDED: &str = "included: borrowed-value\n";

    let mut stack: MyStack<'static> = ParserStack::new();
    stack.set_borrowed_resolver(|name| {
        assert_eq!(name, "borrowed.yaml");
        Ok(INCLUDED)
    });
    stack.push_include("borrowed.yaml").unwrap();

    while let Some(event) = stack.next_event() {
        if let Event::Scalar(value, _, _, _) = event.unwrap().0 {
            if value == "borrowed-value" {
                assert!(matches!(value, Cow::Borrowed("borrowed-value")));
                return;
            }
        }
    }

    panic!("expected the included scalar");
}

#[test]
fn borrowed_include_resolver_still_validates_eagerly() {
    let mut stack: MyStack<'static> = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("root: value\n"), "root".to_string());
    stack.set_borrowed_resolver(|_| Ok("included: [unclosed\n"));

    let error = stack.push_include("invalid.yaml").unwrap_err();

    assert_eq!(
        error.kind(),
        &ErrorKind::UnclosedFlowCollection { open: '[' }
    );
    assert_eq!(error.source_stack(), ["root", "invalid.yaml"]);
}

#[test]
fn empty_stack_include_preserves_fresh_parser_anchor_offset() {
    const INCLUDED: &str = "definition: &anchor anchored\nalias: *anchor\n";

    let mut owned: MyStack<'static> = ParserStack::new();
    owned.set_resolver(|_| Ok(INCLUDED.to_string()));
    owned.push_include("owned.yaml").unwrap();
    let owned_events = collect_events(&mut owned).unwrap();

    assert_eq!(find_anchor_id(&owned_events, "anchored"), Some(1));
    assert!(owned_events
        .iter()
        .any(|event| matches!(event, Event::Alias(1))));

    let mut borrowed: MyStack<'static> = ParserStack::new();
    borrowed.set_borrowed_resolver(|_| Ok(INCLUDED));
    borrowed.push_include("borrowed.yaml").unwrap();
    let borrowed_events = collect_events(&mut borrowed).unwrap();

    assert_eq!(find_anchor_id(&borrowed_events, "anchored"), Some(1));
    assert!(borrowed_events
        .iter()
        .any(|event| matches!(event, Event::Alias(1))));
}

#[test]
fn owned_include_inherits_and_propagates_parent_anchor_offset() {
    let mut stack = stack_after_first_parent_anchor();
    stack.set_resolver(|name| {
        assert_eq!(name, "included.yaml");
        Ok("definition: &included included-value\nalias: *included\n".to_string())
    });

    stack.push_include("included.yaml").unwrap();
    let events = collect_events(&mut stack).unwrap();

    assert_include_anchor_offset_is_inherited(&events);
}

#[test]
fn borrowed_include_inherits_and_propagates_parent_anchor_offset() {
    const INCLUDED: &str = "definition: &included included-value\nalias: *included\n";

    let mut stack = stack_after_first_parent_anchor();
    stack.set_borrowed_resolver(|name| {
        assert_eq!(name, "included.yaml");
        Ok(INCLUDED)
    });

    stack.push_include("included.yaml").unwrap();
    let events = collect_events(&mut stack).unwrap();

    assert_include_anchor_offset_is_inherited(&events);
}

#[test]
fn replay_parser_moves_owned_event_text() {
    let value = "owned replay value".to_string();
    let value_ptr = value.as_ptr();
    let mut replay = ReplayParser::new(
        vec![(
            Event::Scalar(Cow::Owned(value), ScalarStyle::Plain, 0, None),
            test_span(),
        )],
        1,
    );

    let event = replay.next().unwrap().unwrap().0;
    let Event::Scalar(Cow::Owned(value), _, _, _) = event else {
        panic!("expected an owned scalar");
    };

    assert_eq!(value.as_ptr(), value_ptr);
}

#[test]
fn test_unexpected_eof_is_reported() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: [1, 2"), "p1".to_string());

    let res = collect_events(&mut stack);
    assert!(res.is_err(), "expected malformed input to return an error");
}

#[test]
fn test_replay_parser_updates_anchor_offset() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("k1: &a v1\nk3: &c v3"),
        "p1".to_string(),
    );

    loop {
        let ev = stack.next_event().unwrap().unwrap().0;
        if matches!(ev, Event::Scalar(ref val, _, _, _) if val.as_ref() == "v1") {
            break;
        }
    }

    let span = Span::empty(Marker::new(0, 1, 0));
    let replay_events = vec![
        (Event::StreamStart, span),
        (Event::DocumentStart(false, None), span),
        (Event::MappingStart(StructureStyle::Block, 0, None), span),
        (
            Event::Scalar("k2".into(), granit_parser::ScalarStyle::Plain, 0, None),
            span,
        ),
        (
            Event::Scalar("v2".into(), granit_parser::ScalarStyle::Plain, 2, None),
            span,
        ),
        (Event::MappingEnd, span),
        (Event::DocumentEnd, span),
        (Event::StreamEnd, span),
    ];
    stack.push_replay_parser(ReplayParser::new(replay_events, 1), "replay".to_string());

    let events = collect_events(&mut stack).unwrap();
    let v3_ev = events
        .iter()
        .find(|e| matches!(e, Event::Scalar(v, _, _, _) if v.as_ref() == "v3"))
        .unwrap();

    if let Event::Scalar(_, _, id, _) = v3_ev {
        assert_eq!(
            *id, 3,
            "Parent parser should continue after replayed anchors"
        );
    }
}

#[test]
fn test_replay_parser_without_anchors_does_not_regress_anchor_offset() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("k1: &a v1\nk2: &b v2"),
        "parent".to_string(),
    );

    loop {
        let ev = stack.next_event().unwrap().unwrap().0;
        if matches!(ev, Event::Scalar(ref val, _, _, _) if val.as_ref() == "v1") {
            break;
        }
    }

    let span = Span::empty(Marker::new(0, 1, 0));
    let replay_events = vec![
        (Event::StreamStart, span),
        (Event::DocumentStart(false, None), span),
        (Event::MappingStart(StructureStyle::Block, 0, None), span),
        (
            Event::Scalar(
                "included".into(),
                granit_parser::ScalarStyle::Plain,
                0,
                None,
            ),
            span,
        ),
        (
            Event::Scalar("value".into(), granit_parser::ScalarStyle::Plain, 0, None),
            span,
        ),
        (Event::MappingEnd, span),
        (Event::DocumentEnd, span),
        (Event::StreamEnd, span),
    ];

    stack.push_replay_parser(ReplayParser::new(replay_events, 1), "replay".to_string());

    let events = collect_events(&mut stack).unwrap();
    let v2_ev = events
        .iter()
        .find(|e| matches!(e, Event::Scalar(v, _, _, _) if v.as_ref() == "v2"))
        .unwrap();

    if let Event::Scalar(_, _, id, _) = v2_ev {
        assert_eq!(
            *id, 2,
            "Parent parser should not reuse anchor IDs after replay without anchors"
        );
    }
}

#[test]
fn replay_parser_peek_next_and_load_track_collection_anchors() {
    let span = test_span();
    let replay_events = vec![
        (Event::StreamStart, span),
        (Event::DocumentStart(false, None), span),
        (Event::SequenceStart(StructureStyle::Block, 4, None), span),
        (Event::SequenceEnd, span),
        (Event::MappingStart(StructureStyle::Block, 7, None), span),
        (Event::MappingEnd, span),
        (Event::DocumentEnd, span),
        (Event::StreamEnd, span),
    ];
    let mut replay = ReplayParser::new(replay_events, 1);

    assert!(matches!(
        replay.peek().unwrap().unwrap().0,
        Event::StreamStart
    ));
    assert!(matches!(
        replay.next_event().unwrap().unwrap().0,
        Event::StreamStart
    ));

    let mut recv = TestReceiver { events: Vec::new() };
    replay.load(&mut recv, true).unwrap();

    assert_eq!(replay.anchor_offset(), 8);
    assert_eq!(
        format_events(&recv.events),
        vec![
            "DocStart",
            "SeqStart",
            "SeqEnd",
            "MapStart",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn replay_parser_load_single_stops_at_document_end() {
    let span = test_span();
    let replay_events = vec![
        (Event::StreamStart, span),
        (Event::DocumentStart(false, None), span),
        (plain_scalar("first", 0), span),
        (Event::DocumentEnd, span),
        (Event::DocumentStart(false, None), span),
        (plain_scalar("second", 0), span),
        (Event::DocumentEnd, span),
        (Event::StreamEnd, span),
    ];
    let mut replay = ReplayParser::new(replay_events, 1);
    let mut recv = TestReceiver { events: Vec::new() };

    replay.load(&mut recv, false).unwrap();

    assert_eq!(
        format_events(&recv.events),
        vec!["StreamStart", "DocStart", "Scalar(first)", "DocEnd"]
    );
}

#[test]
fn replay_parser_try_load_single_stops_at_document_end() {
    let span = test_span();
    let replay_events = vec![
        (Event::StreamStart, span),
        (Event::DocumentStart(false, None), span),
        (plain_scalar("first", 0), span),
        (Event::DocumentEnd, span),
        (Event::DocumentStart(false, None), span),
        (plain_scalar("second", 0), span),
        (Event::DocumentEnd, span),
        (Event::StreamEnd, span),
    ];
    let mut replay = ReplayParser::new(replay_events, 1);
    let mut recv = TryTestReceiver { events: Vec::new() };

    replay.try_load(&mut recv, false).unwrap();

    assert_eq!(
        format_events(&recv.events),
        vec!["StreamStart", "DocStart", "Scalar(first)", "DocEnd"]
    );
}

#[test]
fn replay_parser_try_load_multi_reads_stream_end() {
    let span = test_span();
    let replay_events = vec![
        (Event::StreamStart, span),
        (Event::DocumentStart(false, None), span),
        (plain_scalar("first", 0), span),
        (Event::DocumentEnd, span),
        (Event::StreamEnd, span),
    ];
    let mut replay = ReplayParser::new(replay_events, 1);
    let mut recv = TryTestReceiver { events: Vec::new() };

    replay.try_load(&mut recv, true).unwrap();

    assert_eq!(
        format_events(&recv.events),
        vec![
            "StreamStart",
            "DocStart",
            "Scalar(first)",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
#[cfg(feature = "comments")]
fn replay_parser_preserves_comment_events() {
    let span = test_span();
    let replay_events = vec![
        (Event::StreamStart, span),
        (Event::Comment(" replay".into(), Placement::Free), span),
        (Event::DocumentStart(false, None), span),
        (plain_scalar("value", 0), span),
        (Event::DocumentEnd, span),
        (Event::StreamEnd, span),
    ];
    let mut replay = ReplayParser::new(replay_events, 1);
    let mut recv = TestReceiver { events: Vec::new() };

    replay.load(&mut recv, true).unwrap();

    assert_eq!(
        format_events(&recv.events),
        vec![
            "StreamStart",
            "Comment( replay)",
            "DocStart",
            "Scalar(value)",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
#[cfg(feature = "comments")]
fn parser_stack_forwards_comment_events_from_stacked_parsers() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("parent: value\n"),
        "parent".to_string(),
    );
    stack.push_str_parser(
        Parser::new_from_str("# child\nchild: value\n"),
        "child".to_string(),
    );

    let events = collect_events(&mut stack).unwrap();
    let names = format_events(&events);

    let child_comment = names
        .iter()
        .position(|event| event == "Comment( child)")
        .expect("child comment should be forwarded");
    let child_map = names
        .iter()
        .position(|event| event == "MapStart")
        .expect("child mapping should be forwarded");

    assert!(child_comment < child_map);
}

#[test]
#[cfg(feature = "comments")]
fn parser_stack_include_preserves_comment_events_and_local_spans() {
    let included = "# inc\nincluded: value\n";
    let mut stack: MyStack = ParserStack::new();
    stack.set_resolver(move |name| {
        assert_eq!(name, "included.yaml");
        Ok(included.to_string())
    });
    stack.push_str_parser(
        Parser::new_from_str("parent: value\n"),
        "parent".to_string(),
    );
    stack
        .push_include("included.yaml")
        .expect("include should resolve");

    let mut events = Vec::new();
    while let Some(event) = stack.next_event() {
        events.push(event.unwrap());
    }

    let (_, span) = events
        .iter()
        .find(|(event, _)| matches!(event, Event::Comment(text, _) if text == " inc"))
        .expect("included comment should be forwarded");

    assert_eq!(span.start.index(), 0);
    assert_eq!(span.end.index(), "# inc".chars().count());
}

#[test]
fn default_empty_stack_peek_and_next_emit_stream_end_once() {
    let mut stack: MyStack = ParserStack::default();

    assert!(matches!(stack.peek().unwrap().unwrap().0, Event::StreamEnd));
    assert!(matches!(stack.peek().unwrap().unwrap().0, Event::StreamEnd));
    assert!(matches!(
        stack.next_event().unwrap().unwrap().0,
        Event::StreamEnd
    ));
    assert!(stack.next_event().is_none());
}

#[test]
fn parser_stack_push_str_after_exhaustion_stays_exhausted() {
    let mut stack: MyStack = ParserStack::new();

    while stack.next_event().is_some() {}
    assert!(stack.peek().is_none());

    stack.push_str_parser(Parser::new_from_str("b: 2"), "second".to_string());

    assert!(stack.next_event().is_none());
    assert!(stack.peek().is_none());
}

#[test]
fn parser_stack_push_after_peeked_empty_stream_end_reactivates_stack() {
    let mut stack: MyStack = ParserStack::new();

    assert!(matches!(stack.peek().unwrap().unwrap().0, Event::StreamEnd));

    stack.push_str_parser(Parser::new_from_str("b: 2"), "second".to_string());

    let events = collect_events(&mut stack).unwrap();
    assert_eq!(
        format_events(&events),
        vec![
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(b)",
            "Scalar(2)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn parser_stack_include_without_resolver_reports_error() {
    let mut stack: MyStack = ParserStack::new();

    let err = stack.push_include("missing").unwrap_err();

    assert!(err
        .to_string()
        .contains("No include resolver set for parser stack."));
}

#[test]
fn parser_stack_resolver_can_construct_a_scan_error() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("root: value"), "root".to_string());
    stack.set_resolver(|_| Err(ScanError::new(Marker::new(0, 1, 0), "include not found")));

    let err = stack.push_include("missing").unwrap_err();

    assert_eq!(
        err.kind(),
        &ErrorKind::Custom("include not found".to_string())
    );
    assert_eq!(
        err.info(),
        "include not found\nwhile parsing root -> missing"
    );
    assert_eq!(err.source_stack(), ["root", "missing"]);
}

#[test]
fn parser_stack_push_include_resolves_included_content() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: 1"), "parent".to_string());
    stack.set_resolver(|name| match name {
        "child" => Ok("b: 2".to_string()),
        _ => unreachable!("this test only resolves child"),
    });

    stack.push_include("child").unwrap();

    let events = collect_events(&mut stack).unwrap();

    assert_eq!(
        format_events(&events),
        vec![
            "MapStart",
            "Scalar(b)",
            "Scalar(2)",
            "MapEnd",
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(a)",
            "Scalar(1)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn parser_stack_push_include_after_exhaustion_stays_exhausted() {
    let mut stack: MyStack = ParserStack::new();
    stack.set_resolver(|name| match name {
        "child" => Ok("b: 2".to_string()),
        _ => unreachable!("this test only resolves child"),
    });

    while stack.next_event().is_some() {}
    assert!(stack.peek().is_none());

    stack.push_include("child").unwrap();

    assert!(stack.next_event().is_none());
    assert!(stack.peek().is_none());
}

#[test]
fn multi_document_include_does_not_splice_into_parent() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("root:\n  inc: !include two.yaml\n  after: tail\n"),
        "root".to_string(),
    );
    stack.set_resolver(|_| Ok("x: 1\n---\ny: 2\n".to_string()));

    let mut saw_error = false;
    let mut spliced = false;

    while let Some(result) = stack.next_event() {
        match result {
            Ok((Event::Scalar(value, _, _, Some(_)), _)) if value.as_ref() == "two.yaml" => {
                stack.push_include(value.as_ref()).unwrap();
            }
            Ok((Event::Scalar(value, ..), _)) if value.as_ref() == "y" => {
                spliced = true;
            }
            Err(err) => {
                assert_eq!(err.kind(), &ErrorKind::MultipleDocumentsUnsupported);
                assert_eq!(
                    err.info(),
                    "multiple documents not supported here\nwhile parsing root -> two.yaml"
                );
                assert_eq!(err.source_stack(), ["root", "two.yaml"]);
                assert_eq!(stack.stack(), vec!["root".to_string()]);
                saw_error = true;
            }
            _ => {}
        }
    }

    assert!(saw_error);
    assert!(
        !spliced,
        "second include document leaked into parent stream"
    );
}

#[test]
fn iter_parser_inherits_anchor_offset_and_reports_stack() {
    let mut stack: ParserStack<'static, alloc::vec::IntoIter<char>, StrInput<'static>> =
        ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("k1: &a v1\nk3: &c v3"),
        "parent".to_string(),
    );

    loop {
        let ev = stack.next_event().unwrap().unwrap().0;
        if matches!(ev, Event::Scalar(ref val, _, _, _) if val.as_ref() == "v1") {
            break;
        }
    }
    assert_eq!(stack.current_anchor_offset(), 2);

    let iter = "k2: &b v2".chars().collect::<Vec<_>>().into_iter();
    stack.push_iter_parser(Parser::new_from_iter(iter), "iter".to_string());

    assert_eq!(
        stack.stack(),
        vec!["parent".to_string(), "iter".to_string()]
    );
    assert_eq!(stack.current_anchor_offset(), 2);

    let events = collect_events(&mut stack).unwrap();
    assert_eq!(find_anchor_id(&events, "v2"), Some(2));
    assert_eq!(find_anchor_id(&events, "v3"), Some(3));
}

#[test]
fn custom_parser_inherits_anchor_offset_and_reports_stack() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("k1: &a v1\nk3: &c v3"),
        "parent".to_string(),
    );

    loop {
        let ev = stack.next_event().unwrap().unwrap().0;
        if matches!(ev, Event::Scalar(ref val, _, _, _) if val.as_ref() == "v1") {
            break;
        }
    }
    assert_eq!(stack.current_anchor_offset(), 2);

    stack.push_custom_parser(
        Parser::new(StrInput::new("k2: &b v2")),
        "custom".to_string(),
    );

    assert_eq!(
        stack.stack(),
        vec!["parent".to_string(), "custom".to_string()]
    );
    assert_eq!(stack.current_anchor_offset(), 2);

    let events = collect_events(&mut stack).unwrap();
    assert_eq!(find_anchor_id(&events, "v2"), Some(2));
    assert_eq!(find_anchor_id(&events, "v3"), Some(3));
}

#[test]
fn custom_parser_with_current_primes_next_event() {
    let mut stack: MyStack = ParserStack::new();
    let span = test_span();
    stack.push_custom_parser_with_current(
        Parser::new(StrInput::new("k: v")),
        "custom".to_string(),
        (plain_scalar("primed", 0), span),
    );

    assert_eq!(stack.stack(), vec!["custom".to_string()]);

    let (event, event_span) = stack.next_event().unwrap().unwrap();
    assert_eq!(event, plain_scalar("primed", 0));
    assert_eq!(event_span, span);
}

#[test]
fn replay_parser_without_stream_end_is_popped_at_eof() {
    let span = test_span();
    let mut stack: MyStack = ParserStack::new();

    stack.push_replay_parser(
        ReplayParser::new(vec![(plain_scalar("only", 0), span)], 1),
        "replay".to_string(),
    );

    assert!(matches!(
        stack.next_event().unwrap().unwrap().0,
        Event::Scalar(..)
    ));
    assert!(matches!(
        stack.next_event().unwrap().unwrap().0,
        Event::StreamEnd
    ));
    assert!(stack.next_event().is_none());
}

#[test]
fn nested_replay_without_stream_end_is_popped_and_parent_continues() {
    let span = test_span();
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("parent: value"), "parent".to_string());
    stack.push_replay_parser(
        ReplayParser::new(vec![(plain_scalar("included", 0), span)], 1),
        "replay".to_string(),
    );

    let events = collect_events(&mut stack).unwrap();

    assert_eq!(
        format_events(&events),
        vec![
            "Scalar(included)",
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(parent)",
            "Scalar(value)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn parser_stack_peek_surfaces_parse_error() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: [1, 2"), "bad".to_string());

    loop {
        match stack.peek() {
            Some(Ok(_)) => {
                stack.next_event().unwrap().unwrap();
            }
            Some(Err(err)) => {
                assert_eq!(err.info(), "unclosed bracket '['");
                break;
            }
            None => panic!("expected parse error before the stream ended"),
        }
    }
}

#[test]
fn parser_stack_error_survives_peek_until_next_event() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: [1, 2"), "bad".to_string());

    loop {
        match stack.peek() {
            Some(Ok(_)) => {
                stack.next_event().unwrap().unwrap();
            }
            Some(Err(err)) => {
                assert_eq!(err.info(), "unclosed bracket '['");
                break;
            }
            None => panic!("expected parse error before the stream ended"),
        }
    }

    let second_peek_error = stack.peek().unwrap().unwrap_err();
    assert_eq!(second_peek_error.info(), "unclosed bracket '['");

    let next_error = stack.next_event().unwrap().unwrap_err();
    assert_eq!(next_error.info(), "unclosed bracket '['");
    assert!(stack.next_event().is_none());
    assert!(stack.peek().is_none());
}

#[test]
fn parser_stack_next_event_error_is_terminal() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: [1, 2"), "bad".to_string());

    loop {
        match stack.next_event() {
            Some(Ok(_)) => {}
            Some(Err(err)) => {
                assert_eq!(err.info(), "unclosed bracket '['");
                break;
            }
            None => panic!("expected parse error before the stream ended"),
        }
    }

    assert!(stack.next_event().is_none());
    assert!(stack.peek().is_none());
}

#[test]
fn nested_replay_stream_end_is_popped_and_parent_continues() {
    let span = test_span();
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("parent: value"), "parent".to_string());
    stack.push_replay_parser(
        ReplayParser::new(vec![(Event::StreamEnd, span)], 1),
        "empty".to_string(),
    );

    let events = collect_events(&mut stack).unwrap();

    assert_eq!(
        format_events(&events),
        vec![
            "StreamStart",
            "DocStart",
            "MapStart",
            "Scalar(parent)",
            "Scalar(value)",
            "MapEnd",
            "DocEnd",
            "StreamEnd"
        ]
    );
}

#[test]
fn parser_stack_peek_after_stream_end_returns_none() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(Parser::new_from_str("a: b"), "p1".to_string());

    while stack.next_event().is_some() {}

    assert!(stack.peek().is_none());
}

#[test]
fn replay_child_propagates_anchor_offset_to_iter_parent() {
    let mut stack: ParserStack<'static, alloc::vec::IntoIter<char>, StrInput<'static>> =
        ParserStack::new();
    let parent = "k1: &a v1\nk3: &c v3"
        .chars()
        .collect::<Vec<_>>()
        .into_iter();
    stack.push_iter_parser(Parser::new_from_iter(parent), "iter-parent".to_string());

    loop {
        let ev = stack.next_event().unwrap().unwrap().0;
        if matches!(ev, Event::Scalar(ref value, _, _, _) if value.as_ref() == "v1") {
            break;
        }
    }

    let span = test_span();
    stack.push_replay_parser(
        ReplayParser::new(
            vec![
                (plain_scalar("included", 2), span),
                (Event::StreamEnd, span),
            ],
            1,
        ),
        "replay".to_string(),
    );

    let events = collect_events(&mut stack).unwrap();

    assert_eq!(find_anchor_id(&events, "included"), Some(2));
    assert_eq!(find_anchor_id(&events, "v3"), Some(3));
}

#[test]
fn replay_child_propagates_anchor_offset_to_custom_parent() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_custom_parser(
        Parser::new(StrInput::new("k1: &a v1\nk3: &c v3")),
        "custom-parent".to_string(),
    );

    loop {
        let ev = stack.next_event().unwrap().unwrap().0;
        if matches!(ev, Event::Scalar(ref value, _, _, _) if value.as_ref() == "v1") {
            break;
        }
    }

    let span = test_span();
    stack.push_replay_parser(
        ReplayParser::new(
            vec![
                (plain_scalar("included", 2), span),
                (Event::StreamEnd, span),
            ],
            1,
        ),
        "replay".to_string(),
    );

    let events = collect_events(&mut stack).unwrap();

    assert_eq!(find_anchor_id(&events, "included"), Some(2));
    assert_eq!(find_anchor_id(&events, "v3"), Some(3));
}

#[test]
fn replay_child_propagates_anchor_offset_to_replay_parent() {
    let span = test_span();
    let mut stack: MyStack = ParserStack::new();
    stack.push_replay_parser(
        ReplayParser::new(vec![(plain_scalar("parent", 0), span)], 1),
        "replay-parent".to_string(),
    );
    stack.push_replay_parser(
        ReplayParser::new(
            vec![(plain_scalar("child", 4), span), (Event::StreamEnd, span)],
            1,
        ),
        "replay-child".to_string(),
    );

    assert_eq!(
        stack.next_event().unwrap().unwrap().0,
        plain_scalar("child", 4)
    );
    assert_eq!(
        stack.next_event().unwrap().unwrap().0,
        plain_scalar("parent", 0)
    );
    assert_eq!(stack.current_anchor_offset(), 5);
}

#[test]
fn custom_parser_with_current_inherits_parent_anchor_offset() {
    let mut stack: MyStack = ParserStack::new();
    stack.push_str_parser(
        Parser::new_from_str("k1: &a v1\nk3: &c v3"),
        "parent".to_string(),
    );

    loop {
        let ev = stack.next_event().unwrap().unwrap().0;
        if matches!(ev, Event::Scalar(ref value, _, _, _) if value.as_ref() == "v1") {
            break;
        }
    }

    let span = test_span();
    stack.push_custom_parser_with_current(
        Parser::new(StrInput::new("k2: &b v2")),
        "custom".to_string(),
        (plain_scalar("primed", 0), span),
    );

    assert_eq!(stack.current_anchor_offset(), 2);
    assert_eq!(
        stack.next_event().unwrap().unwrap().0,
        plain_scalar("primed", 0)
    );

    let events = collect_events(&mut stack).unwrap();
    assert_eq!(find_anchor_id(&events, "v2"), Some(2));
    assert_eq!(find_anchor_id(&events, "v3"), Some(3));
}
