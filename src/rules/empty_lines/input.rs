use crate::rules::indentation::LineGeometry;

#[derive(Clone, Copy)]
pub(crate) struct Line {
    pub(super) origin: usize,
    pub(super) empty: bool,
    pub(super) blank: bool,
    pub(super) terminated: bool,
}

impl Line {
    pub(super) fn retained(index: usize, content: &str, ending: &str) -> Self {
        Self {
            origin: index,
            empty: content.is_empty(),
            blank: content.is_empty(),
            terminated: !ending.is_empty(),
        }
    }
    pub(super) fn projected(line: &LineGeometry<'_>, marker: bool) -> Self {
        Self {
            origin: line.origin(),
            empty: line.empty(),
            blank: line.empty()
                || (marker
                    && line.bom()
                    && line.retained_prefix() == 0
                    && !line.has_body()),
            terminated: line.has_break(),
        }
    }
}

pub(super) struct Stream<'a>(&'a str);

impl<'a> Stream<'a> {
    pub(super) const fn new(text: &'a str) -> Self {
        Self(text)
    }
    pub(super) fn starts_lf(&self) -> bool {
        self.0.starts_with('\n')
    }
    pub(super) fn only_lf(&self) -> bool {
        self.0.bytes().all(|byte| byte == b'\n')
    }
}
