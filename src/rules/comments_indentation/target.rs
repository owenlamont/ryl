use super::{
    LineInfo, LineKind, comment_is_aligned, compute_next_content_indents,
    compute_prev_content_indents,
};

pub(super) struct Trailing<'a> {
    lines: super::RetainedLines<'a>,
    own: super::input::Projected<'a>,
    previous: isize,
    reference: isize,
    first: usize,
}

pub(super) struct TrailingAdmission<'a> {
    own: super::input::Projected<'a>,
    last: usize,
}

impl<'a> TrailingAdmission<'a> {
    pub(super) fn new(
        config: Option<super::Config>,
        last: Option<usize>,
        own: super::input::Projected<'a>,
    ) -> Option<Self> {
        let last = last?;
        (config? == super::Config::default() && own.has_comments_after(last))
            .then_some(Self { own, last })
    }
}

impl<'a> Trailing<'a> {
    pub(super) fn new(
        admission: TrailingAdmission<'a>,
        lines: super::RetainedLines<'a>,
        mut columns: impl DoubleEndedIterator<Item = crate::rules::indentation::Column>,
    ) -> Self {
        let previous = columns
            .rfind(|column| lines.kind(column.origin()) == Some(LineKind::Other))
            .map_or(0, |column| column.current());
        Self {
            lines,
            own: admission.own,
            previous,
            reference: previous,
            first: admission.last + 1,
        }
    }

    pub(super) const fn first(&self) -> usize {
        self.first
    }

    pub(super) fn line(
        &mut self,
        index: usize,
        column: crate::rules::indentation::Column,
    ) -> Option<isize> {
        if !self.own.comment(index) {
            return None;
        }
        match self.lines.kind(column.origin()) {
            Some(LineKind::Comment) => {
                let aligned = if column.current() == 0 {
                    0
                } else {
                    self.reference
                };
                self.reference = aligned;
                Some(column.delta_to(aligned))
            }
            Some(LineKind::ScalarTrailingComment) => {
                self.reference = column.current();
                None
            }
            _ => {
                self.reference = self.previous;
                None
            }
        }
    }
}

pub(super) struct Planner<'a> {
    lines: &'a [LineInfo],
    open: &'a [Vec<usize>],
    previous: Vec<Option<usize>>,
    next: Vec<Option<usize>>,
    last_comment: Option<usize>,
    changed: bool,
}

impl<'a> Planner<'a> {
    pub(super) fn new(lines: &'a [LineInfo], open: &'a [Vec<usize>]) -> Self {
        Self {
            lines,
            open,
            previous: compute_prev_content_indents(lines),
            next: compute_next_content_indents(lines),
            last_comment: None,
            changed: false,
        }
    }

    pub(super) fn line(&mut self, index: usize, tabs: bool) -> Option<usize> {
        let line = &self.lines[index];
        match line.kind {
            LineKind::Comment => {
                let previous = self.previous[index].unwrap_or(0);
                let next = self.next[index].unwrap_or(0);
                let reference = self.last_comment.unwrap_or_else(|| previous.max(next));
                let target = if comment_is_aligned(
                    line.indent,
                    reference,
                    next,
                    self.open.get(index).map_or(&[], Vec::as_slice),
                ) {
                    line.indent
                } else {
                    reference
                };
                self.last_comment = Some(target);
                self.changed |= target != line.indent;
                (target != line.indent || tabs).then_some(target)
            }
            LineKind::ScalarTrailingComment => {
                self.last_comment = Some(line.indent);
                None
            }
            LineKind::Other | LineKind::DirectiveComment => {
                self.last_comment = None;
                None
            }
            LineKind::Empty | LineKind::ScalarBody => None,
        }
    }

    pub(super) fn finish(self) -> bool {
        self.changed
    }
}
