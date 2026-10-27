use ryl_yaml_parser::ScalarStyle;

pub(crate) struct RetainedRegions<'a> {
    input: crate::text_version::Analysis<'a>,
}

impl<'a> RetainedRegions<'a> {
    pub(super) const fn new(input: crate::text_version::Analysis<'a>) -> Self {
        Self { input }
    }

    pub(crate) fn contains(&self, at: usize) -> bool {
        self.input.parsed().scalar_contains(at)
    }
}

pub(crate) struct RetainedShape<'a> {
    buffer: &'a str,
    range: std::ops::Range<usize>,
}

impl<'a> RetainedShape<'a> {
    pub(super) const fn new(buffer: &'a str, range: std::ops::Range<usize>) -> Self {
        Self { buffer, range }
    }

    pub(crate) fn indentation_indicator(&self) -> bool {
        self.buffer[self.range.start + 1..]
            .chars()
            .take_while(|&ch| ch.is_ascii_digit() || matches!(ch, '+' | '-'))
            .any(|ch| ch.is_ascii_digit())
    }

    pub(crate) fn trailing_breaks(&self) -> usize {
        let from = self.buffer[..self.range.start]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at);
        self.buffer[from..self.range.end]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| {
                matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
            })
            .filter(|(at, ch)| {
                *ch == '\n'
                    || (*ch == '\r' && !self.buffer[from + at + 1..].starts_with('\n'))
            })
            .count()
    }
}

#[derive(Clone, Copy)]
pub(super) struct Initial<'a> {
    pub(super) style: ScalarStyle,
    content: &'a str,
}

impl<'a> Initial<'a> {
    pub(super) const fn new(style: ScalarStyle, content: &'a str) -> Self {
        Self { style, content }
    }

    pub(super) fn any_escape(self) -> bool {
        self.content.contains('\\')
    }

    pub(super) fn non_quote_escape(self) -> bool {
        self.content
            .match_indices('\\')
            .any(|(at, _)| !self.content[at + 1..].starts_with('"'))
    }

    pub(super) fn multiline(self) -> bool {
        self.content.contains('\n') || self.content.contains('\r')
    }

    pub(super) fn escaped_line_ending(self) -> bool {
        self.multiline()
            && (self.content.contains("\\\n") || self.content.contains("\\\r\n"))
    }
}

pub(crate) struct LineContent<'a> {
    text: &'a str,
}

impl<'a> LineContent<'a> {
    pub(super) const fn new(text: &'a str) -> Self {
        Self { text }
    }
    pub(crate) const fn empty(&self) -> bool {
        self.text.is_empty()
    }
    pub(crate) fn starts_indent(&self) -> bool {
        self.text.starts_with([' ', '\t'])
    }
    pub(crate) fn blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

#[derive(PartialEq, Eq)]
pub(crate) enum ContentKind {
    Empty,
    Whitespace,
    Text,
}

impl LineContent<'_> {
    pub(crate) fn kind(&self) -> ContentKind {
        if self.empty() {
            ContentKind::Empty
        } else if self.starts_indent() {
            ContentKind::Whitespace
        } else {
            ContentKind::Text
        }
    }
}

pub(crate) struct RetainedTail(bool);

impl RetainedTail {
    pub(super) fn observe(buffer: &str, end: usize) -> Self {
        Self(buffer[..end].ends_with(['\n', '\r']))
    }
    pub(crate) const fn ends_in_break(&self) -> bool {
        self.0
    }
}

impl LineContent<'_> {
    pub(crate) fn horizontal_whitespace(&self) -> bool {
        self.text.bytes().all(|ch| matches!(ch, b' ' | b'\t'))
    }
}
