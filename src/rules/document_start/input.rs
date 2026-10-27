use crate::rules::{comments, document_end};
use crate::text_version::Analysis;
use ryl_yaml_parser::Event;

pub(super) struct Start {
    pub(super) explicit: bool,
    pub(super) line: usize,
    pub(super) column: usize,
}

pub(super) fn starts(source: Analysis<'_>) -> impl Iterator<Item = Start> {
    source.event_iter().filter_map(|(event, span)| {
        if let Event::DocumentStart(explicit, _) = event {
            Some(Start {
                explicit,
                line: span.start.line(),
                column: span.start.col(),
            })
        } else {
            None
        }
    })
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
