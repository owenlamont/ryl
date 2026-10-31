use crate::rules::{comments, document_end};
use crate::text_version::Analysis;
use ryl_yaml_parser::{Event, ScalarStyle, Span};

pub(crate) struct Start {
    pub(crate) explicit: bool,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) declared: bool,
    pub(crate) offset: usize,
    pub(crate) follows_end_marker: bool,
}

pub(crate) fn starts(source: Analysis<'_>) -> impl Iterator<Item = Start> {
    let mut events = source.event_iter().peekable();
    let mut follows_end_marker = true;
    std::iter::from_fn(move || {
        loop {
            let (event, span) = events.next()?;
            let (explicit, version) = match event {
                Event::DocumentStart(explicit, version) => (explicit, version),
                Event::DocumentEnd => {
                    follows_end_marker = !span.is_empty();
                    continue;
                }
                _ => continue,
            };
            // A block scalar's span starts at its content, a line below the header that
            // opens an implicit document.
            let opens_with_block_scalar = !explicit
                && matches!(
                    events.peek(),
                    Some((
                        Event::Scalar(
                            _,
                            ScalarStyle::Literal | ScalarStyle::Folded,
                            ..
                        ),
                        _
                    ))
                );
            let line = if opens_with_block_scalar {
                header_line(source, span)
            } else {
                span.start.line()
            };
            return Some(Start {
                explicit,
                line,
                column: span.start.col(),
                declared: version.is_some(),
                offset: crate::rules::support::span_utils::marker_byte_offset(
                    span.start,
                )
                .get(),
                follows_end_marker,
            });
        }
    })
}

fn header_line(source: Analysis<'_>, span: Span) -> usize {
    let headers = source.headers();
    headers
        .binary_search_by_key(&span.start.index(), |header| header.span.start.index())
        .map_or(span.start.line(), |found| headers[found].line)
}

pub(super) struct Lines<'a> {
    source: Analysis<'a>,
}

impl<'a> Lines<'a> {
    pub(super) const fn new(source: Analysis<'a>) -> Self {
        Self { source }
    }
    pub(super) fn first(&self) -> comments::RetainedFirstLine {
        comments::retained_first_line(self.source.line_content(0))
    }
    pub(super) fn bom(&self, line: usize) -> bool {
        self.source.line_content(line - 1).starts_with('\u{feff}')
    }
    pub(super) fn end(&self, line: usize) -> document_end::RetainedMarker {
        document_end::retained_marker(
            self.source
                .line_content(line - 1)
                .strip_prefix('\u{feff}')
                .unwrap_or(self.source.line_content(line - 1)),
        )
    }
}
