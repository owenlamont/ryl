mod support;
use ryl_yaml_parser::{Event, Options, Parser, ParserStack, Span, StrInput};
#[cfg(feature = "comments")]
use ryl_yaml_parser::{Marker, ParserTrait, Placement, ReplayParser, ScalarStyle};
#[cfg(feature = "comments")]
use std::borrow::Cow;
use std::{boxed::Box, vec::Vec};

type Stack = ParserStack<'static, std::vec::IntoIter<char>, StrInput<'static>>;

fn no_comment_options() -> Options {
    ryl_yaml_parser::options! {
        emit_comments: false,
        #[cfg(feature = "comments")]
        max_buffered_comment_events: 0,
    }
}

#[cfg(feature = "comments")]
fn span() -> Span {
    Span::empty(Marker::new(0, 1, 0))
}

fn ambiguous_comment_run() -> String {
    let mut yaml = String::from("key: # c0\n");
    for index in 1..40 {
        yaml.push_str("# c");
        yaml.push_str(&index.to_string());
        yaml.push('\n');
    }
    yaml.push_str("next: value\n");
    yaml
}

fn assert_has_no_comments(events: &[(Event<'_>, Span)]) {
    assert!(
        events
            .iter()
            .all(|(event, _)| !support::is_comment_event(event)),
        "comment event escaped a no-comments parser stack"
    );
}

const CHILD_WITH_TRAILING_COMMENT: &str = "child\n... # trailing\n";

fn stack_with_parent() -> Stack {
    let mut stack = Stack::with_options(no_comment_options());
    stack.push_str_parser(Parser::new_from_str("parent\n"), "parent".to_owned());
    stack
}

fn assert_nested_trailing_comment_is_suppressed(stack: Stack) {
    let events = stack.collect::<Result<Vec<_>, _>>().unwrap();
    assert_has_no_comments(&events);

    for expected in ["child", "parent"] {
        assert!(events
            .iter()
            .any(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == expected)));
    }
    assert_eq!(
        events
            .iter()
            .filter(|(event, _)| matches!(event, Event::StreamEnd))
            .count(),
        1
    );
}

#[test]
#[cfg(feature = "comments")]
fn default_stack_emits_replayed_comments() {
    let mut stack = Stack::new();
    stack.push_replay_parser(
        ReplayParser::new(
            vec![
                (
                    Event::Comment(Cow::Borrowed(" replay"), Placement::Free),
                    span(),
                ),
                (Event::StreamEnd, span()),
            ],
            1,
        ),
        "replay".to_owned(),
    );

    let first = stack.next_event().unwrap().unwrap().0;
    assert!(matches!(first, Event::Comment(ref text, _) if text == " replay"));
}

#[test]
#[cfg(feature = "comments")]
fn no_comments_stack_suppresses_replayed_events() {
    let mut stack = Stack::with_options(no_comment_options());
    stack.push_replay_parser(
        ReplayParser::new(
            vec![
                (
                    Event::Comment(Cow::Borrowed(" replay"), Placement::Free),
                    span(),
                ),
                (
                    Event::Scalar(Cow::Borrowed("value"), ScalarStyle::Plain, 0, None),
                    span(),
                ),
                (Event::StreamEnd, span()),
            ],
            1,
        ),
        "replay".to_owned(),
    );

    let events = stack.collect::<Result<Vec<_>, _>>().unwrap();
    assert_has_no_comments(&events);
    assert!(events
        .iter()
        .any(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == "value")));
}

#[test]
#[cfg(feature = "comments")]
fn no_comments_stack_suppresses_preconsumed_current_and_later_comments() {
    let mut parser = Parser::new_from_str("# current\n# later\nvalue\n");
    assert!(matches!(
        parser.next_event().unwrap().unwrap().0,
        Event::StreamStart
    ));
    let current = parser.next_event().unwrap().unwrap();
    assert!(matches!(current.0, Event::Comment(..)));

    let mut stack = Stack::with_options(no_comment_options());
    stack.push_custom_parser_with_current(parser, "custom".to_owned(), current);

    let events = stack.collect::<Result<Vec<_>, _>>().unwrap();
    assert_has_no_comments(&events);
    assert!(events
        .iter()
        .any(|(event, _)| matches!(event, Event::Scalar(value, ..) if value == "value")));
}

#[test]
fn no_comments_options_reach_borrowed_includes() {
    let yaml: &'static str = Box::leak(ambiguous_comment_run().into_boxed_str());
    let mut stack = Stack::with_options(no_comment_options());
    stack.set_borrowed_resolver(move |_| Ok(yaml));

    stack.push_include("borrowed.yaml").unwrap();
    let events = stack.collect::<Result<Vec<_>, _>>().unwrap();
    assert_has_no_comments(&events);
    assert_eq!(
        events
            .iter()
            .filter_map(|(event, _)| event.scalar().map(|(value, _)| value))
            .collect::<Vec<_>>(),
        ["key", "~", "next", "value"],
    );
}

#[test]
fn no_comments_options_reach_owned_includes() {
    let yaml = ambiguous_comment_run();
    let mut stack = Stack::with_options(no_comment_options());
    stack.set_resolver(move |_| Ok(yaml.clone()));

    stack.push_include("owned.yaml").unwrap();
    let events = stack.collect::<Result<Vec<_>, _>>().unwrap();
    assert_has_no_comments(&events);
    assert_eq!(
        events
            .iter()
            .filter_map(|(event, _)| event.scalar().map(|(value, _)| value))
            .collect::<Vec<_>>(),
        ["key", "~", "next", "value"],
    );
}

#[test]
fn suppressed_string_comment_after_nested_document_end_is_not_a_second_document() {
    let mut stack = stack_with_parent();
    stack.push_str_parser(
        Parser::new_from_str(CHILD_WITH_TRAILING_COMMENT),
        "string child".to_owned(),
    );

    assert_nested_trailing_comment_is_suppressed(stack);
}

#[test]
fn suppressed_iterator_comment_after_nested_document_end_is_not_a_second_document() {
    let mut stack = stack_with_parent();
    let child = CHILD_WITH_TRAILING_COMMENT
        .chars()
        .collect::<Vec<_>>()
        .into_iter();
    stack.push_iter_parser(Parser::new_from_iter(child), "iterator child".to_owned());

    assert_nested_trailing_comment_is_suppressed(stack);
}

#[test]
fn suppressed_custom_comment_after_nested_document_end_is_not_a_second_document() {
    let mut stack = stack_with_parent();
    stack.push_custom_parser(
        Parser::new_from_str(CHILD_WITH_TRAILING_COMMENT),
        "custom child".to_owned(),
    );

    assert_nested_trailing_comment_is_suppressed(stack);
}

#[test]
#[cfg(feature = "comments")]
fn suppressed_replay_comment_after_nested_document_end_is_not_a_second_document() {
    let mut stack = stack_with_parent();
    stack.push_replay_parser(
        ReplayParser::new(
            vec![
                (
                    Event::Scalar(Cow::Borrowed("child"), ScalarStyle::Plain, 0, None),
                    span(),
                ),
                (Event::DocumentEnd, span()),
                (
                    Event::Comment(Cow::Borrowed(" trailing"), Placement::Last),
                    span(),
                ),
                (Event::StreamEnd, span()),
            ],
            1,
        ),
        "replay child".to_owned(),
    );

    assert_nested_trailing_comment_is_suppressed(stack);
}
