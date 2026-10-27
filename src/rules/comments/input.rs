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
