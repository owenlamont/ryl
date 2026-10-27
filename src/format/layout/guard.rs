use crate::directives::Directives;
use crate::fix::Passes;
use crate::rules::support::span_utils::BytePos;
use crate::text_version::Analysis;

pub(super) struct Guard {
    directives: Directives,
    starts: Vec<usize>,
    last: usize,
    added_starts: Vec<usize>,
}

#[derive(Clone, Copy)]
pub(crate) struct Permission<'a> {
    guard: Option<&'a Guard>,
    rule: &'static str,
}

impl Permission<'_> {
    pub(crate) fn line(self, line: usize) -> bool {
        self.guard.is_none_or(|guard| guard.line(self.rule, line))
    }

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
        let directives = input.directives();
        if passes.per_line.is_empty()
            && !crate::format::FORMAT_RULE_IDS
                .iter()
                .any(|rule| directives.disables_any(rule))
        {
            return None;
        }
        let directives = if passes.per_line.is_empty() {
            directives.clone()
        } else {
            Directives::parse_per_line_with_analysis(input, &passes.per_line)
        };
        Some(Box::new(Self {
            directives,
            starts: input.byte_line_starts().to_vec(),
            last: input.line_count().saturating_sub(1),
            added_starts: Vec::new(),
        }))
    }

    pub(super) fn allows(&self, rule: &str, start: BytePos, end: BytePos) -> bool {
        self.range(start, end).all(|line| self.line(rule, line))
    }

    pub(super) fn allows_any(&self, rule: &str, start: BytePos, end: BytePos) -> bool {
        self.range(start, end).any(|line| self.line(rule, line))
    }

    fn range(&self, start: BytePos, end: BytePos) -> std::ops::RangeInclusive<usize> {
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

    pub(super) fn add_start(&mut self, input: Analysis<'_>, at: BytePos) {
        let line = input
            .byte_line_starts()
            .partition_point(|&start| start <= at.get())
            .saturating_sub(1);
        self.added_starts.push(
            input
                .parsed()
                .document_starts
                .partition_point(|&start| start < line),
        );
    }

    pub(super) fn added_start(&self, document: usize) -> bool {
        self.added_starts.binary_search(&document).is_ok()
    }
}
