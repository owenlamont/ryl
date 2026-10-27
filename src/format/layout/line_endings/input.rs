pub(super) struct Split<'a> {
    buffer: &'a str,
    retained: &'a str,
}

impl<'a> Split<'a> {
    pub(super) const fn new(buffer: &'a str, retained: &'a str) -> Self {
        Self { buffer, retained }
    }

    pub(super) const fn retained(&self) -> &'a str {
        self.retained
    }

    pub(super) fn first_ending(&self) -> &'static str {
        crate::rules::support::line_syntax::buffer_newline(self.buffer)
    }
}
