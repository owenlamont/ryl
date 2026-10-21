#![cfg_attr(not(test), no_main)]
#[cfg(feature = "comments")]
use granit_parser::Placement;

#[cfg(not(test))]
mod common;
#[cfg(test)]
use crate::common;

use std::fmt::Write;

use granit_parser::{
    ErrorKind, Event, Marker, Parser, ParserStack, ParserTrait, ReplayParser, ScalarStyle,
    ScanError, Span, StrInput,
};
#[cfg(not(test))]
use libfuzzer_sys::fuzz_target;

type Stack<'a> = ParserStack<'a, std::vec::IntoIter<char>, StrInput<'a>>;

const PARENT: &str = "parent\n";
const MIDDLE: &str = "middle\n... # middle tail\n";

#[cfg(not(test))]
fuzz_target!(|data: &[u8]| check_input(data));

/// The first five bytes select backend, tail, comment count, line ending, and nested insertion.
/// Remaining bytes become bounded printable comment text; no UTF-8 input is required.
///
/// Every generated source is checked against explicit scalar/comment/error expectations, with
/// comments both enabled and disabled and with both direct reads and repeated peeks.
///
/// # Panics
/// Panics if a generated source cannot be prepared or parser-stack events, errors, comment
/// placement, or repeated peeks violate the independent expectations.
#[allow(clippy::too_many_lines)] // Keep each generated source and its independent oracle together.
pub fn check_input(data: &[u8]) {
    if data.len() > 512 {
        return;
    }
    let selector = |index| data.get(index).copied().unwrap_or(0);
    let tail = selector(1) % 5;
    // ReplayParser cannot carry scan errors, and resolvers eagerly parse an include before
    // pushing it. Malformed-tail cases exercise errors during live stack iteration instead.
    let backend = selector(0) % if tail == 4 { 3 } else { 7 };
    let newline = ["\n", "\r\n", "\r"][usize::from(selector(3) % 3)];
    let nested = selector(4) & 1 != 0;
    let comment_count = if tail == 4 { 0 } else { selector(2) % 5 };
    let payload: String = data
        .get(5..)
        .unwrap_or_default()
        .iter()
        .map(|byte| match byte {
            b' '..=b'~' => char::from(*byte),
            b'\t' => '\t',
            128..=191 => 'é',
            192..=255 => '🪨',
            _ => '_',
        })
        .collect();

    let mut source = format!("child{newline}... # tail {payload}{newline}");
    let mut expected = vec![
        scalar("child"),
        #[cfg(feature = "comments")]
        Event::Comment(format!(" tail {payload}").into(), Placement::Right),
    ];
    for index in 0..comment_count {
        let text = format!(" after {index} {payload}");
        source.push('#');
        source.push_str(&text);
        source.push_str(newline);
        #[cfg(feature = "comments")]
        expected.push(Event::Comment(
            text.into(),
            // Placement::Last requires no further token before StreamEnd. A following
            // comment is itself a token, so consecutive earlier comments remain Above.
            if tail == 0 && index + 1 == comment_count {
                Placement::Last
            } else {
                Placement::Above
            },
        ));
    }

    let expected_error = match tail {
        0 => {
            expected.extend(parent_events());
            None
        }
        1..=3 => {
            match tail {
                1 => write!(source, "---{newline}second{newline}").unwrap(),
                2 => source.push_str("---"),
                _ => write!(source, "second{newline}").unwrap(),
            }
            Some((
                ErrorKind::MultipleDocumentsUnsupported,
                Marker::new(5 + newline.chars().count(), 2, 0),
            ))
        }
        _ => {
            let opening = Marker::new(source.chars().count(), 3, 0);
            source.push('[');
            source.push_str(newline);
            Some((ErrorKind::UnclosedFlowCollection { open: '[' }, opening))
        }
    };

    for &emit_comments in common::comment_modes() {
        for peek in [false, true] {
            let mut stack = Stack::with_options(granit_parser::options! {
                #[cfg(feature = "comments")]
                emit_comments: emit_comments,
            });
            stack.push_str_parser(Parser::new_from_str(PARENT), "parent.yaml".into());
            if nested {
                stack.push_str_parser(Parser::new_from_str(MIDDLE), "middle.yaml".into());
                assert_eq!(
                    next_checked(&mut stack, peek).unwrap().unwrap().0,
                    scalar("middle")
                );
                #[cfg(feature = "comments")]
                if emit_comments {
                    // Suspend the middle source after its DocumentEnd has been consumed,
                    // leaving its trailing-comment validation pending during the child.
                    assert_eq!(
                        next_checked(&mut stack, peek).unwrap().unwrap().0,
                        Event::Comment(" middle tail".into(), Placement::Right)
                    );
                }
            }
            push_child(&mut stack, backend, &source);

            for event in expected.iter().filter(|event| {
                (cfg!(feature = "comments") && emit_comments) || !common::is_comment_event(event)
            }) {
                let (actual, _) = next_checked(&mut stack, peek)
                    .expect("generated source ended before its expected events")
                    .expect("generated source failed before its expected events");
                assert_eq!(
                    &actual, event,
                    "backend={backend}, tail={tail}, source={source:?}"
                );
            }

            if let Some((kind, marker)) = &expected_error {
                let error = next_checked(&mut stack, peek)
                    .expect("included source failure was mistaken for EOF")
                    .expect_err("an included source emitted content after its document ended");
                assert_eq!(error.kind(), kind);
                assert_eq!(error.marker(), marker);
                let mut sources = vec!["parent.yaml"];
                if nested {
                    sources.push("middle.yaml");
                }
                sources.push("child.yaml");
                assert_eq!(error.source_stack(), sources);
                sources.pop();
                assert_eq!(stack.stack(), sources);
            } else {
                assert_eq!(stack.stack(), Vec::<String>::new());
            }
            for _ in 0..3 {
                assert!(stack.next_event().is_none(), "stack did not fuse");
                assert!(stack.peek().is_none(), "peek revived a finished stack");
                assert!(stack.next().is_none(), "Iterator revived a finished stack");
            }
        }
    }
}

fn scalar(value: &'static str) -> Event<'static> {
    Event::Scalar(value.into(), ScalarStyle::Plain, 0, None)
}

fn parent_events() -> [Event<'static>; 5] {
    [
        Event::StreamStart,
        Event::DocumentStart(false, None),
        scalar("parent"),
        Event::DocumentEnd,
        Event::StreamEnd,
    ]
}

fn next_checked<'a>(
    stack: &mut Stack<'a>,
    peek: bool,
) -> Option<Result<(Event<'a>, Span), ScanError>> {
    if peek {
        let first = stack.peek().map(Result::<&_, _>::cloned);
        assert_eq!(stack.peek().map(Result::<&_, _>::cloned), first);
        let next = stack.next_event();
        assert_eq!(next, first, "peek and next_event disagree");
        next
    } else {
        stack.next_event()
    }
}

fn push_child<'a>(stack: &mut Stack<'a>, backend: u8, source: &'a str) {
    match backend {
        0 => stack.push_str_parser(Parser::new_from_str(source), "child.yaml".into()),
        1 => stack.push_iter_parser(
            Parser::new_from_iter(source.chars().collect::<Vec<_>>().into_iter()),
            "child.yaml".into(),
        ),
        2 => stack.push_custom_parser(Parser::new(StrInput::new(source)), "child.yaml".into()),
        3 | 4 => {
            let mut events = Parser::new_from_str(source)
                .collect::<Result<Vec<_>, _>>()
                .expect("generated replay input must be valid YAML");
            if backend == 4 {
                assert!(matches!(events.pop().unwrap().0, Event::StreamEnd));
            }
            stack.push_replay_parser(ReplayParser::new(events, 1), "child.yaml".into());
        }
        5 => {
            stack.set_resolver(move |_| Ok(source.to_owned()));
            stack
                .push_include("child.yaml")
                .expect("valid owned include");
        }
        6 => {
            stack.set_borrowed_resolver(move |_| Ok(source));
            stack
                .push_include("child.yaml")
                .expect("valid borrowed include");
        }
        _ => unreachable!("backend selector is bounded"),
    }
}
