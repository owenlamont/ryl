use super::PlanEdit;
use crate::text_version::Analysis;

pub(crate) struct RetainedFragment<'a> {
    text: &'a str,
}

impl<'a> RetainedFragment<'a> {
    pub(crate) const fn observe(text: &'a str) -> Self {
        Self { text }
    }
    pub(crate) fn characters(&self) -> usize {
        self.text.chars().count()
    }
    pub(crate) fn multiline(&self) -> bool {
        self.text.contains(['\n', '\r'])
    }
    pub(crate) const fn empty(&self) -> bool {
        self.text.is_empty()
    }
}

pub(crate) struct Projection {
    starts: Vec<usize>,
    ends: Vec<usize>,
    deltas: Vec<isize>,
    bytes: Vec<isize>,
    lines: Vec<(usize, usize)>,
    queries: std::cell::Cell<usize>,
}

impl Projection {
    pub(crate) fn incremental() -> Self {
        Self {
            starts: Vec::new(),
            ends: Vec::new(),
            deltas: vec![0],
            bytes: Vec::new(),
            lines: Vec::new(),
            queries: std::cell::Cell::new(0),
        }
    }

    pub(crate) fn append(
        &mut self,
        input: Analysis<'_>,
        edits: &[crate::rules::support::token_spacing::SpaceSite],
    ) {
        if !edits.is_empty() && self.lines.is_empty() {
            self.lines.resize(input.byte_line_starts().len(), (0, 0));
        }
        for site in edits {
            let (from, to, text) = site.edit;
            let line = input
                .byte_line_starts()
                .partition_point(|&at| at <= to.get())
                - 1;
            if self.lines[line].0 == self.lines[line].1 {
                self.lines[line].0 = self.ends.len();
            }
            self.ends.push(to.get());
            self.lines[line].1 = self.ends.len();
            let old = input.text()[from.get()..to.get()].chars().count();
            let delta = isize::try_from(text).expect("text fits isize")
                - isize::try_from(old).expect("text fits isize");
            self.deltas
                .push(self.deltas.last().copied().unwrap_or(0) + delta);
        }
    }

    pub(crate) fn new(input: Analysis<'_>, edits: &[PlanEdit]) -> Self {
        if edits.is_empty() {
            return Self {
                starts: Vec::new(),
                ends: Vec::new(),
                deltas: Vec::new(),
                bytes: Vec::new(),
                lines: Vec::new(),
                queries: std::cell::Cell::new(0),
            };
        }
        let edit_starts = edits.iter().map(|(start, _, _)| start.get()).collect();
        let ends: Vec<_> = edits.iter().map(|(_, end, _)| end.get()).collect();
        let mut deltas = Vec::with_capacity(edits.len() + 1);
        let mut bytes = Vec::with_capacity(edits.len() + 1);
        deltas.push(0);
        bytes.push(0);
        for (from, to, text) in edits {
            let old = input.text()[from.get()..to.get()].chars().count();
            let delta = isize::try_from(text.char_count()).expect("text fits isize")
                - isize::try_from(old).expect("text fits isize");
            deltas.push(deltas.last().copied().unwrap_or(0) + delta);
            let delta = isize::try_from(text.len()).expect("text fits isize")
                - isize::try_from(to.get() - from.get()).expect("text fits isize");
            bytes.push(bytes.last().copied().unwrap_or(0) + delta);
        }
        let (mut first, mut last) = (0, 0);
        let starts = input.byte_line_starts();
        let lines = starts
            .iter()
            .enumerate()
            .map(|(line, &start)| {
                while first < ends.len() && ends[first] < start {
                    first += 1;
                }
                let end = starts.get(line + 1).copied().unwrap_or(input.text().len());
                while last < ends.len() && ends[last] <= end {
                    last += 1;
                }
                (first, last)
            })
            .collect();
        Self {
            starts: edit_starts,
            ends,
            deltas,
            bytes,
            lines,
            queries: std::cell::Cell::new(0),
        }
    }

    pub(crate) fn column(&self, line: usize, byte: usize, column: usize) -> usize {
        self.queries.set(self.queries.get() + 1);
        if self.lines.is_empty() {
            return column;
        }
        let (first, last) = self.lines[line];
        let stop = first + self.ends[first..last].partition_point(|&end| end <= byte);
        column.saturating_add_signed(self.deltas[stop] - self.deltas[first])
    }

    pub(crate) fn byte_position(
        &self,
        input: Analysis<'_>,
        start: usize,
        stop: usize,
        position: usize,
    ) -> usize {
        self.queries.set(self.queries.get() + 1);
        let position = position + stop - start;
        if self.lines.is_empty() {
            return position;
        }
        let line = input.byte_line_starts().partition_point(|&at| at <= start) - 1;
        let (first, last) = self.lines[line];
        let from = first + self.starts[first..last].partition_point(|&at| at < start);
        let to = first + self.ends[first..last].partition_point(|&at| at <= stop);
        position.saturating_add_signed(self.bytes[to] - self.bytes[from.min(to)])
    }
}

impl Drop for Projection {
    fn drop(&mut self) {
        super::stats::add(super::stats::Counter::Project, self.queries.get());
    }
}
