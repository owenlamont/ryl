mod support;
#[cfg(feature = "comments")]
use granit_parser::Placement;
use granit_parser::{
    ErrorKind, Event, Marker, Options, Parser, ParserStack, ParserTrait, ReplayParser, ScalarStyle,
    ScanError, Span, StrInput,
};

type Stack = ParserStack<'static, std::vec::IntoIter<char>, StrInput<'static>>;

const CHILD: &str = "child\n... # trailing\n# after\n";
const TWO_DOCUMENTS: &str = "child\n... # trailing\n# after\n---\nsecond\n";
const PARENT: &str = "parent\n";

#[derive(Clone, Copy, Debug)]
enum Backend {
    String,
    Iterator,
    Custom,
    Replay,
    ReplayWithoutStreamEnd,
    OwnedInclude,
    BorrowedInclude,
}

const BACKENDS: [Backend; 7] = [
    Backend::String,
    Backend::Iterator,
    Backend::Custom,
    Backend::Replay,
    Backend::ReplayWithoutStreamEnd,
    Backend::OwnedInclude,
    Backend::BorrowedInclude,
];

fn ordinary_events(yaml: &'static str) -> Vec<(Event<'static>, Span)> {
    Parser::new_from_str(yaml)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn nested_events(yaml: &'static str) -> Vec<(Event<'static>, Span)> {
    let mut document_starts = 0;
    ordinary_events(yaml)
        .into_iter()
        .take_while(|(event, _)| {
            if matches!(event, Event::DocumentStart(..)) {
                document_starts += 1;
            }
            document_starts < 2
        })
        .filter(|(event, _)| {
            !matches!(
                event,
                Event::StreamStart
                    | Event::StreamEnd
                    | Event::DocumentStart(..)
                    | Event::DocumentEnd
            )
        })
        .collect()
}

fn stack_with_child(backend: Backend, yaml: &'static str, options: Options) -> Stack {
    let mut stack = Stack::with_options(options);
    stack.push_str_parser(Parser::new_from_str(PARENT), "parent.yaml".to_owned());
    match backend {
        Backend::String => {
            stack.push_str_parser(Parser::new_from_str(yaml), "child.yaml".to_owned());
        }
        Backend::Iterator => {
            let chars = yaml.chars().collect::<Vec<_>>().into_iter();
            stack.push_iter_parser(Parser::new_from_iter(chars), "child.yaml".to_owned());
        }
        Backend::Custom => {
            stack.push_custom_parser(Parser::new(StrInput::new(yaml)), "child.yaml".to_owned());
        }
        Backend::Replay | Backend::ReplayWithoutStreamEnd => {
            let mut events = ordinary_events(yaml);
            if matches!(backend, Backend::ReplayWithoutStreamEnd) {
                assert!(matches!(events.pop().unwrap().0, Event::StreamEnd));
            }
            stack.push_replay_parser(ReplayParser::new(events, 1), "child.yaml".to_owned());
        }
        Backend::OwnedInclude => {
            stack.set_resolver(move |_| Ok(yaml.to_owned()));
            stack.push_include("child.yaml").unwrap();
        }
        Backend::BorrowedInclude => {
            stack.set_borrowed_resolver(move |_| Ok(yaml));
            stack.push_include("child.yaml").unwrap();
        }
    }
    stack
}

fn next_with_repeated_peek(stack: &mut Stack) -> Option<Result<(Event<'static>, Span), ScanError>> {
    let peeked = stack.peek().map(Result::<&_, _>::cloned);
    assert_eq!(stack.peek().map(Result::<&_, _>::cloned), peeked);
    let next = stack.next_event();
    assert_eq!(next, peeked);
    next
}

fn assert_multiple_documents_error(stack: &mut Stack, error: &ScanError) {
    assert_eq!(error.kind(), &ErrorKind::MultipleDocumentsUnsupported);
    assert_eq!(*error.marker(), Marker::new(6, 2, 0));
    assert_eq!(error.source_stack(), ["parent.yaml", "child.yaml"]);
    assert_eq!(stack.stack(), ["parent.yaml"]);
    assert!(stack.next_event().is_none());
    assert!(stack.peek().is_none());
}

#[test]
#[cfg(feature = "comments")]
fn included_trailing_comments_are_emitted_before_parent_resumes() {
    // Keep the semantic oracle independent of the parser used by every backend.
    let expected = vec![
        Event::Scalar("child".into(), ScalarStyle::Plain, 0, None),
        Event::Comment(" trailing".into(), Placement::Right),
        Event::Comment(" after".into(), Placement::Last),
        Event::StreamStart,
        Event::DocumentStart(false, None),
        Event::Scalar("parent".into(), ScalarStyle::Plain, 0, None),
        Event::DocumentEnd,
        Event::StreamEnd,
    ];

    for backend in BACKENDS {
        let mut stack = stack_with_child(backend, CHILD, Options::default());
        let mut actual = Vec::new();
        while let Some(event) = next_with_repeated_peek(&mut stack) {
            actual.push(event.unwrap());
        }
        assert_eq!(
            actual
                .iter()
                .map(|(event, _)| event.clone())
                .collect::<Vec<_>>(),
            expected,
            "{backend:?}"
        );
        assert_eq!(
            actual[1].1,
            Span::new(Marker::new(10, 2, 4), Marker::new(20, 2, 14)),
            "{backend:?}: inline comment span"
        );
        assert_eq!(
            actual[2].1,
            Span::new(Marker::new(21, 3, 0), Marker::new(28, 3, 7)),
            "{backend:?}: following comment span"
        );
        assert_eq!(stack.stack(), Vec::<String>::new());
    }
}

#[test]
fn trailing_comments_do_not_hide_a_second_included_document() {
    for backend in BACKENDS {
        for &emit_comments in support::comment_modes() {
            let options = granit_parser::options! { #[cfg(feature = "comments")] emit_comments: emit_comments };
            let mut stack = stack_with_child(backend, TWO_DOCUMENTS, options);
            let mut actual = Vec::new();
            let error = loop {
                match next_with_repeated_peek(&mut stack).expect("second document must fail") {
                    Ok(event) => actual.push(event),
                    Err(error) => break error,
                }
            };
            let expected = vec![
                Event::Scalar("child".into(), ScalarStyle::Plain, 0, None),
                #[cfg(feature = "comments")]
                Event::Comment(" trailing".into(), Placement::Right),
                #[cfg(feature = "comments")]
                Event::Comment(" after".into(), Placement::Above),
            ]
            .into_iter()
            .filter(|event| {
                (cfg!(feature = "comments") && emit_comments) || !support::is_comment_event(event)
            })
            .collect::<Vec<_>>();
            assert_eq!(
                actual
                    .into_iter()
                    .map(|(event, _)| event)
                    .collect::<Vec<_>>(),
                expected,
                "{backend:?}, emit_comments={emit_comments}"
            );
            assert_multiple_documents_error(&mut stack, &error);
        }
    }
}

#[test]
fn replayed_trailing_comments_do_not_allow_events_after_document_end() {
    for unexpected in [
        Event::Scalar("extra".into(), ScalarStyle::Plain, 0, None),
        Event::StreamStart,
        Event::DocumentEnd,
    ] {
        for &emit_comments in support::comment_modes() {
            let mut events = ordinary_events(CHILD);
            assert!(matches!(events.pop().unwrap().0, Event::StreamEnd));
            events.push((unexpected.clone(), Span::empty(Marker::new(29, 4, 0))));

            let mut stack = Stack::with_options(granit_parser::options! {
                #[cfg(feature = "comments")]
                emit_comments: emit_comments,
            });
            stack.push_str_parser(Parser::new_from_str(PARENT), "parent.yaml".to_owned());
            stack.push_replay_parser(ReplayParser::new(events, 1), "child.yaml".to_owned());

            let expected = [
                Event::Scalar("child".into(), ScalarStyle::Plain, 0, None),
                #[cfg(feature = "comments")]
                Event::Comment(" trailing".into(), Placement::Right),
                #[cfg(feature = "comments")]
                Event::Comment(" after".into(), Placement::Last),
            ];
            for event in expected.into_iter().filter(|event| {
                (cfg!(feature = "comments") && emit_comments) || !support::is_comment_event(event)
            }) {
                assert_eq!(
                    next_with_repeated_peek(&mut stack).unwrap().unwrap().0,
                    event
                );
            }

            let error = next_with_repeated_peek(&mut stack).unwrap().unwrap_err();
            assert_multiple_documents_error(&mut stack, &error);
        }
    }
}

#[test]
#[cfg(feature = "comments")]
fn nested_second_document_error_uses_its_own_pending_end_and_source() {
    const GRANDCHILD: &str = "grandchild\n... # grandchild tail\n---\nextra\n";

    for backend in BACKENDS {
        let mut stack = stack_with_child(backend, CHILD, Options::default());
        assert_eq!(
            next_with_repeated_peek(&mut stack).unwrap().unwrap().0,
            Event::Scalar("child".into(), ScalarStyle::Plain, 0, None)
        );
        assert_eq!(
            next_with_repeated_peek(&mut stack).unwrap().unwrap().0,
            Event::Comment(" trailing".into(), Placement::Right)
        );
        // Both sources now need document-end validation; the grandchild's failure must
        // retain its own end marker instead of using the suspended child's marker.
        stack.push_str_parser(
            Parser::new_from_str(GRANDCHILD),
            "grandchild.yaml".to_owned(),
        );
        for expected in [
            Event::Scalar("grandchild".into(), ScalarStyle::Plain, 0, None),
            Event::Comment(" grandchild tail".into(), Placement::Right),
        ] {
            assert_eq!(
                next_with_repeated_peek(&mut stack).unwrap().unwrap().0,
                expected
            );
        }

        let error = next_with_repeated_peek(&mut stack).unwrap().unwrap_err();
        assert_eq!(error.kind(), &ErrorKind::MultipleDocumentsUnsupported);
        assert_eq!(*error.marker(), Marker::new(11, 2, 0));
        assert_eq!(
            error.source_stack(),
            ["parent.yaml", "child.yaml", "grandchild.yaml"]
        );
        assert_eq!(stack.stack(), ["parent.yaml", "child.yaml"]);
        assert!(stack.next_event().is_none());
        assert!(stack.peek().is_none());
    }
}

#[test]
#[cfg(feature = "comments")]
fn pushing_during_a_peeked_trailing_comment_preserves_each_sources_validation() {
    const GRANDCHILD: &str = "grandchild\n... # grandchild tail\n";

    for child in [CHILD, TWO_DOCUMENTS] {
        let mut stack = stack_with_child(Backend::String, child, Options::default());
        let expected_child = nested_events(child);
        assert_eq!(stack.next_event().unwrap().unwrap(), expected_child[0]);

        assert_eq!(stack.peek().unwrap().unwrap(), &expected_child[1]);
        assert_eq!(stack.peek().unwrap().unwrap(), &expected_child[1]);
        stack.push_str_parser(
            Parser::new_from_str(GRANDCHILD),
            "grandchild.yaml".to_owned(),
        );
        assert_eq!(stack.next_event().unwrap().unwrap(), expected_child[1]);

        let mut expected = nested_events(GRANDCHILD);
        expected.extend(expected_child.into_iter().skip(2));
        for event in expected {
            assert_eq!(next_with_repeated_peek(&mut stack).unwrap().unwrap(), event);
        }

        if child == TWO_DOCUMENTS {
            let error = next_with_repeated_peek(&mut stack).unwrap().unwrap_err();
            assert_multiple_documents_error(&mut stack, &error);
        } else {
            let mut resumed = Vec::new();
            while let Some(event) = next_with_repeated_peek(&mut stack) {
                resumed.push(event.unwrap());
            }
            assert_eq!(resumed, ordinary_events(PARENT));
        }
    }
}

#[test]
fn scan_errors_after_trailing_comments_keep_their_kind_and_source() {
    const INVALID_TAIL: &str = "child\n... # trailing\n[\n";

    for backend in [Backend::String, Backend::Iterator, Backend::Custom] {
        let mut stack = stack_with_child(backend, INVALID_TAIL, Options::default());
        let expected = nested_events(CHILD);
        assert_eq!(stack.next_event().unwrap().unwrap(), expected[0]);
        if cfg!(feature = "comments") {
            assert_eq!(
                next_with_repeated_peek(&mut stack).unwrap().unwrap(),
                expected[1]
            );
        }

        let error = next_with_repeated_peek(&mut stack).unwrap().unwrap_err();
        assert_eq!(
            error.kind(),
            &ErrorKind::UnclosedFlowCollection { open: '[' }
        );
        assert_eq!(error.source_stack(), ["parent.yaml", "child.yaml"]);
        assert_eq!(stack.stack(), ["parent.yaml"]);
        assert!(stack.next_event().is_none());
        assert!(stack.peek().is_none());
    }
}
