use super::input::Initial;
use crate::rules::indentation::LineGeometry;

pub(crate) struct Requirement<'a>(Initial<'a>);
pub(crate) struct Selected(&'static str);

impl<'a> Requirement<'a> {
    pub(super) const fn new(initial: Initial<'a>) -> Self {
        Self(initial)
    }
    pub(crate) fn enabled(&self) -> bool {
        self.0.enabled()
    }
    pub(crate) fn select(
        &self,
        mut lines: impl DoubleEndedIterator<Item = LineGeometry<'a>>,
    ) -> Option<Selected> {
        if !self.enabled() {
            return None;
        }
        lines
            .rfind(|line| {
                line.has_break()
                    || line.has_body()
                    || line.has_tail()
                    || line.bom()
                    || line.projected_prefix() > 0
            })
            .filter(|line| !line.has_break())
            .map(|_| Selected(self.0.ending()))
    }
}

impl Selected {
    pub(crate) const fn materialize(self) -> &'static str {
        self.0
    }
}
