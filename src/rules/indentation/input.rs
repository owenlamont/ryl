use crate::format::layout::VirtualLine;

#[derive(Clone, Copy)]
pub(super) struct MappingSite {
    line: usize,
    column: usize,
}

impl MappingSite {
    pub(super) fn observe(start: ryl_yaml_parser::Marker) -> Self {
        Self {
            line: start.line() - 1,
            column: start.col(),
        }
    }

    pub(super) const fn line(self) -> usize {
        self.line
    }

    pub(super) const fn column(self) -> usize {
        self.column
    }
}

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
    pub(super) fn scalar(
        self,
        token: &super::Token,
    ) -> crate::rules::quoted_strings::RetainedShape<'a> {
        crate::rules::quoted_strings::retained_shape(self.text, token.start..token.end)
    }
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

    pub(super) fn prefix(self, index: usize) -> usize {
        self.lines.expect("target input has projected lines")[index].prefix
    }
}
