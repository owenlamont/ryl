use std::ops::Range;

use super::{Kind, Token};
use crate::format::layout::frame::{Chunk, Frame, Placed, Symbol};
use crate::rules::support::collection_style::{Form, Structure};
use crate::text_version::Analysis;
use crate::text_version::model::Container;

fn generated(placed: Placed) -> Token {
    let kind = match placed.symbol {
        Symbol::Empty(style) => Kind::Scalar { style, empty: true },
        Symbol::Structure(structure) => match structure {
            Structure::Start { kind, form, .. } => match (kind, form) {
                (Container::Sequence, Form::Block) => Kind::BlockSequenceStart,
                (Container::Mapping, Form::Block) => Kind::BlockMappingStart,
                (Container::Sequence, Form::Flow) => Kind::FlowSequenceStart,
                (Container::Mapping, Form::Flow) => Kind::FlowMappingStart,
            },
            Structure::End { kind, form, .. } => match (kind, form) {
                (_, Form::Block) => Kind::BlockEnd,
                (Container::Sequence, Form::Flow) => Kind::FlowSequenceEnd,
                (Container::Mapping, Form::Flow) => Kind::FlowMappingEnd,
            },
            Structure::Entry(_) => Kind::BlockEntry,
            Structure::Key(_) => Kind::Key { explicit: false },
            Structure::Value(_) => Kind::Value,
        },
    };
    Token {
        kind,
        start: placed.anchor,
        end: placed.anchor,
        line: placed.first.line,
        column: placed.first.column,
        end_line: placed.last.line,
        end_column: placed.last.column,
    }
}

pub(super) fn tokens<'a>(
    frame: &'a Frame,
    input: Analysis<'a>,
    tokens: impl Iterator<Item = Token> + 'a,
) -> impl Iterator<Item = Token> + 'a {
    let mut containers = Vec::new();
    let mut original = tokens
        .filter(move |token| {
            let hidden = match token.kind {
                Kind::BlockMappingStart
                | Kind::BlockSequenceStart
                | Kind::FlowMappingStart
                | Kind::FlowSequenceStart => {
                    let hidden = frame.form_at(token.start).is_some();
                    containers.push(hidden);
                    hidden
                }
                Kind::BlockEnd | Kind::FlowMappingEnd | Kind::FlowSequenceEnd => {
                    containers
                        .pop()
                        .expect("a normalized collection end has a start")
                }
                Kind::Key { .. } | Kind::Value | Kind::BlockEntry => {
                    containers.last().copied().unwrap_or(false)
                }
                _ => false,
            };
            !hidden
        })
        .peekable();
    let mut chunks = frame.chunks().iter();
    let mut copying: Option<(&Range<usize>, bool)> = None;
    let mut positions = frame.positions();
    std::iter::from_fn(move || {
        loop {
            if let Some((range, unchanged)) = copying {
                while original
                    .peek()
                    .is_some_and(|token| token.start < range.start)
                {
                    original.next();
                }
                let inside = original.peek().is_some_and(|token| {
                    token.start < range.end
                        || (range.is_empty() && token.start == range.start && unchanged)
                        || (unchanged
                            && range.end == input.text().len()
                            && token.start == range.end)
                });
                if !inside {
                    copying = None;
                    continue;
                }
                let mut token = original.next().expect("a copied token was inspected");
                let first = positions.position(input, token.start, token.column, false);
                let last = positions.position(input, token.end, token.end_column, true);
                token.line = first.line;
                token.column = first.column;
                token.end_line = last.line;
                token.end_column = last.column;
                return Some(token);
            }
            match chunks.next()? {
                Chunk::Generated(placed) => return Some(generated(*placed)),
                Chunk::Source { range, original } => {
                    copying = Some((range, *original));
                }
            }
        }
    })
}
