use crate::format::layout::VirtualLine;
use std::ops::Range;

#[derive(Clone, Copy)]
pub(super) struct Initial<'a> {
    text: &'a str,
    starts: &'a [usize],
    lines: Option<&'a [VirtualLine<'a>]>,
}

#[derive(Clone, Copy)]
pub(super) struct GapInitial<'a> {
    lines: &'a [VirtualLine<'a>],
}

impl<'a> GapInitial<'a> {
    pub(super) const fn new(lines: &'a [VirtualLine<'a>]) -> Self {
        Self { lines }
    }

    pub(super) fn after(self, line: usize, column: usize) -> usize {
        self.lines[line].spaces_after(column)
    }
}

impl<'a> Initial<'a> {
    pub(super) fn new(text: &'a str, starts: &'a [usize]) -> Self {
        Self {
            text,
            starts,
            lines: None,
        }
    }

    pub(super) fn projected(mut self, lines: &'a [VirtualLine<'a>]) -> Self {
        self.lines = Some(lines);
        self
    }

    pub(super) fn len(self) -> usize {
        self.starts.len()
    }

    pub(super) fn projected_len(self) -> usize {
        self.lines.expect("target input has projected lines").len()
    }

    pub(super) fn scalar_body_indent(self, index: usize) -> Option<usize> {
        let start = self.starts[index];
        let indent = super::count_spaces(self.text, start);
        (!super::char_at(self.text, start + indent).is_some_and(super::is_break))
            .then_some(indent)
    }

    pub(super) fn continuation_indent(self, index: usize) -> Option<usize> {
        let line = &self.lines.expect("target input has projected lines")[index];
        let content = line.body.trim_start();
        (!content.is_empty()).then_some(line.prefix + line.body.len() - content.len())
    }

    pub(super) fn empty_tail(self, index: usize) -> bool {
        let line = &self.lines.expect("target input has projected lines")[index];
        line.ending.is_empty() && line.body.is_empty() && !line.bom
    }

    pub(super) fn diagnostic_bom(self, index: usize) -> bool {
        self.text[..self.starts[index]].ends_with('\u{feff}')
    }

    pub(super) fn block_column(self, at: usize) -> isize {
        let start = self.text[..at].rfind(['\n', '\r']).map_or(0, |at| at + 1);
        super::to_isize(
            self.text[start..at]
                .chars()
                .filter(|&ch| ch != '\u{feff}')
                .count(),
        )
    }

    pub(super) fn indentation_indicator(self, start: usize) -> bool {
        self.text[start + 1..]
            .chars()
            .take_while(|&ch| ch.is_ascii_digit() || matches!(ch, '+' | '-'))
            .any(|ch| ch.is_ascii_digit())
    }

    pub(super) fn trailing_breaks(self, range: Range<usize>) -> usize {
        let from = self.text[..range.start]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at);
        self.text[from..range.end]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| {
                matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
            })
            .filter(|(at, ch)| {
                *ch == '\n'
                    || (*ch == '\r'
                        && super::char_at(self.text, from + at + 1) != Some('\n'))
            })
            .count()
    }

    pub(super) fn prefix(self, index: usize) -> usize {
        self.lines.expect("target input has projected lines")[index].prefix
    }
}
