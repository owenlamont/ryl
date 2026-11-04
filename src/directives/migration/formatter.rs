use ryl_yaml_parser::{Event, Placement, Span, StructureStyle};

use crate::rules::support::span_utils::{
    BytePos, apply_replacements, marker_byte_offset,
};
use crate::text_version::{Analysis, lexical::Kind};

#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum FormatterComments<'a> {
    None,
    Prettier,
    PrettyYaml(&'a str),
    Yamlfmt,
}

pub struct Migration {
    pub converted: usize,
    pub output: Option<String>,
    pub retained_lines: Vec<usize>,
}

struct Frame {
    mapping: bool,
    flow: bool,
    children: usize,
    column: usize,
}

pub(super) fn migrate(
    input: Analysis<'_>,
    source: FormatterComments<'_>,
    permit: bool,
) -> Migration {
    let context = Context {
        input,
        source,
        tokens: input
            .tokens()
            .iter(input.text())
            .filter(|(span, kind)| {
                matches!(kind, Kind::DocumentStart | Kind::DocumentEnd | Kind::Other)
                    || *kind == Kind::Key && span.slice(input.text()) == Some("?")
            })
            .collect(),
    };
    let comments =
        crate::rules::support::comments_scan::collect_comments_with_analysis(input);
    let native = comments.iter().any(|comment| {
        comment.placement != Placement::Right
            && matches!(
                comment.text.trim_end(),
                " ryl format-ignore" | " ryl format-disable" | " ryl format-enable"
            )
    });
    let blocked = !permit
        || native
        || input.parsed().strict_error().is_some()
        || context.tokens.iter().any(|(_, kind)| *kind == Kind::Key);
    let mut retained_lines = Vec::new();
    let mut replacements = Vec::new();
    let mut candidates = Vec::new();
    for comment in comments {
        let marker = match source {
            FormatterComments::None => None,
            FormatterComments::Prettier => Some("prettier-ignore"),
            FormatterComments::PrettyYaml(marker) => Some(marker),
            FormatterComments::Yamlfmt => Some("!yamlfmt!:ignore"),
        };
        let Some(marker) = marker.filter(|marker| !marker.is_empty()) else {
            continue;
        };
        let payload = comment.text.trim_matches([' ', '\t']);
        if !payload.starts_with(marker) {
            continue;
        }
        let first = input.byte_line_starts()[comment.span.start.line() - 1];
        let at = marker_byte_offset(comment.span.start).get();
        let own_line = input.text()[first..at]
            .chars()
            .all(|character| matches!(character, ' ' | '\t' | '\u{feff}'));
        let lint_directive = super::super::directive_scope(&comment.text).is_some()
            || comment.span.start.line() == 1
                && super::super::disables_file(input.text());
        if payload != marker
            || comment.placement == Placement::Right
            || !own_line
            || blocked
            || lint_directive
        {
            retained_lines.push(comment.span.start.line());
            continue;
        }
        candidates.push(comment.span);
    }
    let mut accepted = Vec::new();
    if matches!(source, FormatterComments::Yamlfmt) {
        collect_file(&context, candidates, &mut accepted, &mut retained_lines);
    } else {
        collect_nodes(&context, &candidates, &mut accepted, &mut retained_lines);
    }
    let legacy = super::legacy_uses(input);
    for (span, replacement) in accepted {
        let first = marker_byte_offset(span.start).get() + 1;
        let last = marker_byte_offset(span.end).get();
        replacements.push((
            BytePos::new(first),
            BytePos::new(last),
            replacement.to_owned(),
        ));
    }
    replacements.extend(
        legacy
            .into_iter()
            .filter(|usage| !retained_lines.contains(&usage.line))
            .map(super::LegacyUse::replacement),
    );
    retained_lines.sort_unstable();
    retained_lines.dedup();
    Migration {
        converted: replacements.len(),
        output: (!replacements.is_empty())
            .then(|| apply_replacements(input.text(), replacements)),
        retained_lines,
    }
}

struct Context<'text, 'marker> {
    input: Analysis<'text>,
    source: FormatterComments<'marker>,
    tokens: Vec<(Span, Kind<'text>)>,
}

impl Context<'_, '_> {
    fn allows(
        &self,
        marker: Span,
        span: Span,
        event: &Event<'_>,
        stack: &[Frame],
        column: usize,
        first_node: bool,
    ) -> bool {
        let parent = stack.last();
        let presentation = parent
            .filter(|frame| !frame.mapping || frame.children.is_multiple_of(2))
            .map_or(column, |frame| frame.column);
        let boundaries: Vec<_> = self
            .tokens
            .iter()
            .filter(|(boundary, _)| {
                boundary.start.line() > marker.start.line()
                    && boundary.start.line() <= span.start.line()
            })
            .collect();
        let initial_pretty_header =
            matches!(self.source, FormatterComments::PrettyYaml(_))
                && first_node
                && boundaries.len() == 1
                && boundaries[0].1 == Kind::DocumentStart
                && !self.tokens.iter().any(|(boundary, kind)| {
                    boundary.start.line() < marker.start.line()
                        && matches!(kind, Kind::DocumentStart | Kind::DocumentEnd)
                });
        let pretty_attachment =
            !matches!(self.source, FormatterComments::PrettyYaml(_))
                || parent.is_none()
                    && (!self.tokens.iter().any(|(boundary, kind)| {
                        *kind == Kind::DocumentStart
                            && boundary.start.line() < marker.start.line()
                    }))
                || parent.is_some_and(|frame| {
                    frame.children > 0
                        && (!frame.mapping || frame.children.is_multiple_of(2))
                });
        (boundaries.is_empty() || initial_pretty_header)
            && pretty_attachment
            && marker.start.col() <= presentation
            && !stack.iter().any(|frame| frame.flow)
            && event.anchor_id().is_none()
            && event.tag().is_none()
    }
}

fn collect_file(
    context: &Context<'_, '_>,
    candidates: Vec<Span>,
    accepted: &mut Vec<(Span, &'static str)>,
    retained_lines: &mut Vec<usize>,
) {
    for span in candidates {
        let single_document = context
            .input
            .event_iter()
            .filter(|(event, _)| matches!(event, Event::DocumentStart(..)))
            .count()
            <= 1
            && !context
                .tokens
                .iter()
                .any(|(_, kind)| *kind == Kind::DocumentEnd);
        if span.start.line() == 1 && single_document {
            accepted.push((span, " ryl format-disable"));
        } else {
            retained_lines.push(span.start.line());
        }
    }
}

fn collect_nodes(
    context: &Context<'_, '_>,
    candidates: &[Span],
    accepted: &mut Vec<(Span, &'static str)>,
    retained_lines: &mut Vec<usize>,
) {
    let mut pending: Option<Span> = None;
    let mut stack: Vec<Frame> = Vec::new();
    let mut first_node = true;
    let mut headers =
        crate::rules::indentation::block_collection_headers(context.input).into_iter();
    for (event, span) in context.input.event_iter() {
        match event {
            Event::Comment(_, _) => {
                if let Some(previous) = pending.take() {
                    retained_lines.push(previous.start.line());
                }
                if candidates.iter().any(|candidate| candidate == &span) {
                    pending = Some(span);
                }
            }
            Event::MappingEnd | Event::SequenceEnd => {
                if let Some(frame) = stack.pop()
                    && pending
                        .as_ref()
                        .is_some_and(|pending| pending.start.col() >= frame.column)
                {
                    retained_lines.push(
                        pending
                            .take()
                            .expect("pending marker was checked")
                            .start
                            .line(),
                    );
                }
            }
            event if event.is_node() => {
                let block = matches!(
                    event,
                    Event::MappingStart(StructureStyle::Block, ..)
                        | Event::SequenceStart(StructureStyle::Block, ..)
                );
                let column = if block {
                    let at =
                        headers.next().expect("each block collection has a header");
                    let start = context.input.text()[..at]
                        .rfind(['\n', '\r'])
                        .map_or(0, |at| at + 1);
                    context.input.text()[start..at].chars().count()
                } else {
                    let start = context.input.byte_line_starts()[span.start.line() - 1];
                    context.input.text()[start..]
                        .chars()
                        .take_while(|character| matches!(character, ' ' | '\t'))
                        .count()
                };
                if pending
                    .as_ref()
                    .is_some_and(|pending| pending.start.line() < span.start.line())
                {
                    let marker = pending.take().expect("pending marker was checked");
                    if context.allows(marker, span, &event, &stack, column, first_node)
                    {
                        accepted.push((marker, " ryl format-ignore"));
                    } else {
                        retained_lines.push(marker.start.line());
                    }
                }
                first_node = false;
                if let Some(parent) = stack.last_mut() {
                    parent.children += 1;
                }
                if let Event::MappingStart(style, ..)
                | Event::SequenceStart(style, ..) = event
                {
                    stack.push(Frame {
                        mapping: matches!(event, Event::MappingStart(..)),
                        flow: style == StructureStyle::Flow,
                        children: 0,
                        column,
                    });
                }
            }
            _ => {}
        }
    }
    if let Some(pending) = pending {
        retained_lines.push(pending.start.line());
    }
}
