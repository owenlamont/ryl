use super::{Cause, Gap};
use crate::directives::Directives;
use crate::rules::comments_indentation::RetainedLine;

pub(super) struct Refusals<'a> {
    starts: &'a [usize],
    permissions: &'a Directives,
    causes: Vec<Option<Cause>>,
}

impl<'a> Refusals<'a> {
    pub(super) fn new(starts: &'a [usize], permissions: &'a Directives) -> Self {
        Self {
            starts,
            permissions,
            causes: vec![None; starts.len()],
        }
    }

    fn refuse(&mut self, line: usize, cause: Cause) {
        let document = self.starts.partition_point(|&start| start <= line) - 1;
        self.causes[document].get_or_insert(cause);
    }

    pub(super) fn line(&mut self, line: usize) {
        if self.permissions.is_disabled(super::ID, line + 1) {
            self.refuse(line, Cause::Disabled);
        }
    }

    pub(super) fn gap(&mut self, gap: Gap, origin: usize) {
        if self.permissions.is_disabled(gap.rule(), origin + 1) {
            self.refuse(origin, Cause::Disabled);
        }
    }

    pub(super) fn needs_protection(
        &self,
        line: usize,
        candidate: TabCandidate,
        trimmed: bool,
    ) -> bool {
        candidate.needs_protection(trimmed)
            && !self
                .permissions
                .is_disabled(crate::rules::trailing_spaces::ID, line + 1)
    }

    pub(super) fn tab(
        &mut self,
        line: usize,
        candidate: TabCandidate,
        trimmed: bool,
        protection: Option<&crate::rules::trailing_spaces::Protection>,
    ) {
        let protected = self
            .permissions
            .is_disabled(crate::rules::trailing_spaces::ID, line + 1)
            || protection.is_some_and(|protection| protection.line(line + 1));
        if let Some(cause) = candidate.resolve(trimmed, protected) {
            self.refuse(line, cause);
        }
    }

    pub(super) fn unfollowable(&mut self, line: usize) {
        self.refuse(line, Cause::Unfollowable);
    }

    pub(super) fn block_tail(
        &mut self,
        line: usize,
        comment: RetainedLine,
        delta: isize,
        body: isize,
        content: Option<usize>,
    ) {
        if let Some(column) = comment.column() {
            let column = super::to_isize(column);
            let content = content.map_or(column + 1, super::to_isize);
            if column + delta >= content + body {
                self.refuse(line, Cause::Changed);
            }
        }
    }

    pub(super) fn finish(self) -> Vec<Option<Cause>> {
        self.causes
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Content {
    first: Option<char>,
    second: Option<char>,
    rest_nonblank: bool,
}

impl Content {
    pub(super) fn extend(&mut self, chars: impl Iterator<Item = char>) {
        for ch in chars {
            if self.first.is_none() {
                if ch != ' ' {
                    self.first = Some(ch);
                }
            } else {
                self.second.get_or_insert(ch);
                self.rest_nonblank |= !matches!(ch, ' ' | '\t');
            }
        }
    }

    fn nonblank(self) -> bool {
        self.first.is_some_and(|ch| ch != '\t') || self.rest_nonblank
    }

    fn tab_in_indentation(self) -> bool {
        self.first == Some('\t')
            || (matches!(self.first, Some('-' | '?' | ':'))
                && self.second == Some('\t')
                && self.rest_nonblank)
    }

    pub(super) fn tab_candidate(mut self, tail: Self) -> Option<TabCandidate> {
        self.rest_nonblank |= tail.nonblank();
        (self.tab_in_indentation() || tail.tab_in_indentation()).then_some(
            TabCandidate {
                blank: !self.nonblank(),
            },
        )
    }
}

#[derive(Clone, Copy)]
pub(super) struct TabCandidate {
    blank: bool,
}

impl TabCandidate {
    pub(super) fn needs_protection(self, trimmed: bool) -> bool {
        self.blank && trimmed
    }

    pub(super) fn resolve(self, trimmed: bool, protected: bool) -> Option<Cause> {
        (!self.needs_protection(trimmed) || protected).then_some(Cause::Tab)
    }
}
