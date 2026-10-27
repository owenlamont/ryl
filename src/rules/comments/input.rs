pub(super) struct Magic<'a> {
    buffer: &'a str,
}

impl<'a> Magic<'a> {
    pub(super) const fn new(buffer: &'a str) -> Self {
        Self { buffer }
    }

    pub(super) fn end(self) -> Option<usize> {
        let first = self.buffer.lines().next().unwrap_or_default();
        crate::rules::support::line_syntax::is_magic_first_line(
            first.strip_prefix('\u{feff}').unwrap_or(first),
        )
        .then_some(first.len())
    }
}

pub(crate) struct DirectiveTail<'a> {
    source: crate::text_version::Analysis<'a>,
    end: crate::rules::support::span_utils::BytePos,
}

impl<'a> DirectiveTail<'a> {
    pub(super) const fn new(
        source: crate::text_version::Analysis<'a>,
        end: crate::rules::support::span_utils::BytePos,
    ) -> Self {
        Self { source, end }
    }
    pub(crate) fn inline(&self) -> bool {
        self.source.text()[self.end.get()..]
            .split(['\r', '\n'])
            .next()
            .is_some_and(has_directive)
    }
    pub(crate) fn guarded(&self) -> bool {
        let origin = self
            .source
            .byte_line_starts()
            .partition_point(|&at| at < self.end.get())
            .saturating_sub(1);
        self.source
            .line_content(origin)
            .get(self.end.get() - self.source.byte_line_starts()[origin]..)
            .is_some_and(has_directive)
    }
}

fn has_directive(text: &str) -> bool {
    text.split_once('#').is_some_and(|(_, comment)| {
        crate::directives::directive_scope(comment).is_some()
    })
}
