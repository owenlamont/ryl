use crate::text_version::Analysis;
use ryl_yaml_parser::Event;

pub(super) enum Kind {
    End { explicit: bool },
    StreamEnd,
    Other,
}

pub(super) struct Boundary {
    pub(super) kind: Kind,
    pub(super) line: usize,
    pub(super) column: usize,
}

pub(super) fn boundaries(source: Analysis<'_>) -> impl Iterator<Item = Boundary> {
    source.event_iter().map(|(event, span)| Boundary {
        kind: match event {
            Event::DocumentEnd => Kind::End {
                explicit: !span.is_empty(),
            },
            Event::StreamEnd => Kind::StreamEnd,
            _ => Kind::Other,
        },
        line: span.start.line(),
        column: span.start.col(),
    })
}

pub(crate) struct RetainedMarker(bool);

impl RetainedMarker {
    pub(super) fn observe(line: &str) -> Self {
        Self(line.split_whitespace().next() == Some("..."))
    }
    pub(crate) const fn present(&self) -> bool {
        self.0
    }
}

pub(super) struct Eof<'a> {
    source: Analysis<'a>,
    at: crate::rules::support::span_utils::BytePos,
}

impl<'a> Eof<'a> {
    pub(super) const fn new(
        source: Analysis<'a>,
        at: crate::rules::support::span_utils::BytePos,
    ) -> Self {
        Self { source, at }
    }
    pub(super) fn unterminated(&self) -> bool {
        self.at.get() == self.source.text().len()
            && !self.source.text().ends_with(['\n', '\r'])
    }
    pub(super) fn ending(&self) -> &'static str {
        crate::rules::support::line_syntax::buffer_newline(self.source.text())
    }
}
