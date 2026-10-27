use crate::rules::block_scalar_chomping::Header;
use crate::text_version::Analysis;

pub(super) struct Blocks<'a> {
    source: Analysis<'a>,
}

impl<'a> Blocks<'a> {
    pub(super) fn new(source: Analysis<'a>) -> Self {
        Self { source }
    }
    pub(super) fn valid(&self) -> bool {
        self.source.parsed().valid
    }
    pub(super) fn count(&self) -> usize {
        self.source.line_count()
    }
    pub(super) fn ends_in_break(&self) -> bool {
        self.source.text().ends_with(['\n', '\r'])
    }
    pub(super) fn blocks(&self) -> impl Iterator<Item = Block<'a>> {
        self.source.headers().iter().map(|header| Block {
            source: self.source,
            header,
        })
    }
    pub(super) fn scalar_ranges(&self) -> &[(usize, usize)] {
        self.source.scalar_lines()
    }
    pub(super) fn scalars(&self) -> impl Iterator<Item = Scalar> {
        self.source.parsed().scalars().map(|(style, span)| Scalar {
            block: matches!(
                style,
                ryl_yaml_parser::ScalarStyle::Literal
                    | ryl_yaml_parser::ScalarStyle::Folded
            ),
            escaped: style == ryl_yaml_parser::ScalarStyle::DoubleQuoted,
            multiline: span.end.line() > span.start.line(),
            start: span
                .start
                .byte_offset()
                .expect("stored scalar has a byte offset"),
        })
    }
    pub(super) fn content_blank(&self, line: usize) -> bool {
        self.source.content_blank_lines().contains(&line)
    }
    pub(super) fn line(&self, line: usize) -> Option<Line<'a>> {
        self.source.get_line(line - 1).map(|text| Line { text })
    }
}

pub(super) struct Scalar {
    pub(super) block: bool,
    pub(super) escaped: bool,
    pub(super) multiline: bool,
    pub(super) start: usize,
}

pub(super) struct Line<'a> {
    text: &'a str,
}

impl Line<'_> {
    pub(super) fn spaces(&self) -> bool {
        self.text.bytes().all(|ch| ch == b' ')
    }
    pub(super) fn bytes(&self) -> usize {
        self.text.len()
    }
    pub(super) fn blank_tabs(&self) -> bool {
        self.text.contains('\t') && self.text.trim_matches([' ', '\t']).is_empty()
    }
    pub(super) fn empty(&self) -> bool {
        self.text.is_empty()
    }
}

pub(super) struct Block<'a> {
    source: Analysis<'a>,
    header: &'a Header,
}

impl Block<'_> {
    pub(super) fn header(&self) -> usize {
        self.header.line
    }
    pub(super) fn start(&self) -> usize {
        self.header.span.start.index()
    }
    pub(super) fn start_line(&self) -> usize {
        self.header.span.start.line()
    }
    pub(super) fn end_line(&self) -> usize {
        self.header.span.end.line()
    }
    pub(super) fn end_column(&self) -> usize {
        self.header.span.end.col()
    }
    pub(super) fn empty(&self) -> bool {
        self.header.span.is_empty()
    }
    pub(super) fn chomp(&self) -> Option<char> {
        self.header.chomping
    }
    pub(super) fn indent(&self) -> Option<usize> {
        self.header.span.indent
    }
    pub(super) fn start_column(&self) -> usize {
        self.header.span.start.col()
    }
    pub(super) fn root(&self) -> bool {
        self.source.parsed().root_scalars.get(&self.start())
            == Some(&self.header.span.end.index())
    }
    pub(super) fn indicator(&self) -> Option<u32> {
        self.source
            .line_content(self.header.line - 1)
            .chars()
            .skip(self.header.column)
            .take_while(|ch| !ch.is_whitespace() && *ch != '#')
            .find_map(|ch| ch.to_digit(10))
    }
    pub(super) fn followed(&self) -> bool {
        self.header
            .span
            .end
            .byte_offset()
            .is_some_and(|offset| !self.source.text()[offset..].trim().is_empty())
    }
    pub(super) fn preceding_spaces(&self) -> bool {
        self.source
            .line_contents_from(self.header.line)
            .take(self.end_line() - 1 - self.header.line)
            .all(|line| line.bytes().all(|ch| ch == b' '))
    }
    pub(super) fn compact_indent(&self) -> usize {
        let line = self.source.line_content(self.header.line - 1);
        let mut rest = line.trim_start_matches(' ');
        while let Some(after) = rest
            .strip_prefix('-')
            .filter(|after| after.starts_with(' '))
        {
            rest = after.trim_start_matches(' ');
        }
        if rest
            .split(['|', '>'])
            .next()
            .is_some_and(|key| key.contains(": "))
        {
            line.len() - rest.len()
        } else {
            line.len() - line.trim_start_matches(' ').len()
        }
    }
    pub(super) fn literal(&self) -> bool {
        self.source
            .line_content(self.header.line - 1)
            .chars()
            .nth(self.header.column - 1)
            == Some('|')
    }
    pub(super) fn following_blanks(&self) -> usize {
        self.source
            .line_contents_from(self.header.line)
            .take_while(|line| line.trim().is_empty())
            .count()
    }
    pub(super) fn next_token(&self) -> bool {
        self.source
            .get_line(self.end_line() - 1)
            .is_some_and(|line| {
                line.chars().take(self.end_column()).all(|ch| ch == ' ')
                    && line.chars().nth(self.end_column()).is_some()
            })
    }
}

impl<'a> Blocks<'a> {
    pub(super) fn last(&self) -> Option<Block<'a>> {
        self.source.headers().last().map(|header| Block {
            source: self.source,
            header,
        })
    }
    pub(super) fn final_whitespace_width(&self) -> Option<usize> {
        self.source
            .text()
            .rsplit(['\n', '\r'])
            .next()
            .filter(|line| line.trim().is_empty())
            .map(str::len)
            .filter(|&width| width > 0)
    }
    pub(super) fn last_block_rooted(&self) -> bool {
        use ryl_yaml_parser::{Event, ScalarStyle};
        self.source
            .event_values()
            .zip(self.source.event_values().skip(1))
            .filter(|(_, event)| {
                matches!(
                    event,
                    Event::Scalar(_, ScalarStyle::Literal | ScalarStyle::Folded, ..)
                )
            })
            .last()
            .is_some_and(|(before, _)| matches!(before, Event::DocumentStart(..)))
    }
    pub(super) fn trailing_blanks(&self) -> usize {
        self.source
            .line_contents_from(0)
            .rev()
            .take_while(|line| line.trim_matches([' ', '\t']).is_empty())
            .take(2)
            .count()
    }
}

impl Block<'_> {
    pub(super) fn next_line_blank(&self) -> bool {
        self.source
            .get_line(self.header.line)
            .is_none_or(|line| line.trim().is_empty())
    }
    pub(super) fn break_indicator(&self) -> Option<usize> {
        self.source
            .line_content(self.header.line - 1)
            .chars()
            .skip(self.header.column)
            .find(|&ch| ch != '+')
            .and_then(|ch| ch.to_digit(10))
            .map(|digit| digit as usize)
    }
    pub(super) fn folded(&self) -> bool {
        self.source
            .line_content(self.header.line - 1)
            .chars()
            .nth(self.header.column - 1)
            == Some('>')
    }
    pub(super) fn header_position(&self) -> crate::rules::support::span_utils::BytePos {
        crate::rules::support::span_utils::BytePos::new(
            self.source.byte_line_starts()[self.header.line - 1],
        )
    }
}
