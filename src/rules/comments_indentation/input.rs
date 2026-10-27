use crate::format::layout::VirtualLine;

pub(super) struct Projected<'a> {
    lines: &'a [VirtualLine<'a>],
}

impl<'a> Projected<'a> {
    pub(super) const fn new(lines: &'a [VirtualLine<'a>]) -> Self {
        Self { lines }
    }

    pub(super) fn has_comments_after(&self, last: usize) -> bool {
        self.lines[last + 1..]
            .iter()
            .any(|line| super::retained_line(line).column().is_some())
    }

    pub(super) fn comment(&self, line: usize) -> bool {
        super::retained_line(&self.lines[line]).column().is_some()
    }
}
