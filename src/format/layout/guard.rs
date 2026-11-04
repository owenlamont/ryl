use super::frame::{LineMap, View};
use crate::directives::Directives;
use crate::fix::Passes;
use crate::rules::support::span_utils::BytePos;
use crate::text_version::Analysis;

pub(super) struct Guard {
    directives: Directives,
    starts: Vec<usize>,
    last: usize,
    projected: Option<LineMap>,
}

#[derive(Clone, Copy)]
pub(crate) struct Permission<'a> {
    guard: Option<&'a Guard>,
    rule: &'static str,
}

impl Permission<'_> {
    pub(crate) fn allows(self, start: BytePos, end: BytePos) -> bool {
        self.guard
            .is_none_or(|guard| guard.allows(self.rule, start, end))
    }
}

impl Guard {
    pub(super) const fn permission<'a>(
        guard: Option<&'a Self>,
        rule: &'static str,
    ) -> Permission<'a> {
        Permission { guard, rule }
    }

    pub(super) fn new(input: Analysis<'_>, passes: &Passes<'_>) -> Option<Box<Self>> {
        let directives = Directives::scoped(input, passes.directive_scope);
        if passes.per_line.is_empty()
            && !crate::format::FORMAT_RULE_IDS
                .iter()
                .any(|rule| directives.disables_any(rule))
        {
            return None;
        }
        let directives = if passes.per_line.is_empty() {
            directives
        } else {
            Directives::scoped_per_line(input, &passes.per_line, passes.directive_scope)
        };
        Some(Box::new(Self {
            directives,
            starts: input.byte_line_starts().to_vec(),
            last: input.line_count().saturating_sub(1),
            projected: None,
        }))
    }

    pub(super) fn projected(
        input: Analysis<'_>,
        passes: &Passes<'_>,
        view: &View<'_, '_>,
    ) -> Option<Box<Self>> {
        let mut guard = Self::new(input, passes)?;
        let (frame, lines) = view.lines();
        guard.directives = Directives::parse_format_projected(input, frame, lines);
        guard.projected = Some(frame.line_map());
        guard.last = lines.len().saturating_sub(1);
        Some(guard)
    }

    pub(super) fn allows(&self, rule: &str, start: BytePos, end: BytePos) -> bool {
        self.directives
            .allows_format_prefix_edit(start.get(), end.get())
            && self.range(start, end).all(|line| self.line(rule, line))
    }

    pub(super) fn allows_line(&self, rule: &str, at: BytePos) -> bool {
        self.range(at, at).all(|line| self.line(rule, line))
    }

    pub(super) fn allows_any(&self, rule: &str, start: BytePos, end: BytePos) -> bool {
        self.range(start, end).any(|line| self.line(rule, line))
    }

    fn range(&self, start: BytePos, end: BytePos) -> std::ops::RangeInclusive<usize> {
        if let Some(projected) = &self.projected {
            let first = projected
                .source_line(&self.starts, start.get())
                .min(self.last);
            let last = projected
                .source_line(&self.starts, end.get().saturating_sub(1).max(start.get()))
                .min(self.last);
            return first..=last;
        }
        let first = self
            .starts
            .partition_point(|&at| at <= start.get())
            .saturating_sub(1)
            .min(self.last);
        let last = self
            .starts
            .partition_point(|&at| at <= end.get().saturating_sub(1).max(start.get()))
            .saturating_sub(1)
            .min(self.last);
        first..=last
    }

    pub(super) fn line(&self, rule: &str, line: usize) -> bool {
        !self.directives.is_disabled(rule, line + 1)
    }
}
