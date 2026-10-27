use crate::format::layout::VirtualLine;
use crate::rules::comments::RetainedPrefix;
use crate::rules::support::span_utils::marker_byte_offset;
use crate::text_version::Analysis;

pub(crate) struct CollectionPrefix {
    compact: bool,
    width: usize,
    dashes: usize,
    outer: usize,
}

impl CollectionPrefix {
    pub(super) fn observe(prefix: &str) -> Self {
        let compact = prefix
            .split(' ')
            .all(|token| token.is_empty() || token == "-");
        let dashes = prefix.trim_end_matches(' ');
        let outer = if compact {
            dashes.strip_suffix('-').unwrap_or(dashes)
        } else {
            prefix
        };
        Self {
            compact,
            width: prefix.len(),
            dashes: dashes.len(),
            outer: outer.len(),
        }
    }

    pub(crate) const fn compact(&self) -> bool {
        self.compact
    }
    pub(crate) const fn width(&self) -> usize {
        self.width
    }
    pub(crate) const fn dashes(&self) -> usize {
        self.dashes
    }
    pub(crate) const fn outer(&self) -> usize {
        self.outer
    }
}

pub(crate) struct RetainedDash(bool);

impl RetainedDash {
    pub(super) fn observe(
        buffer: &str,
        at: crate::rules::support::span_utils::BytePos,
    ) -> Self {
        Self(buffer[..at.get()].ends_with('-'))
    }

    pub(crate) const fn present(&self) -> bool {
        self.0
    }
}

pub(crate) struct EntrySite {
    inline: Option<usize>,
    above: Option<(usize, RetainedPrefix)>,
}

impl EntrySite {
    pub(super) fn observe(input: Analysis<'_>, start: ryl_yaml_parser::Marker) -> Self {
        let line = start.line() - 1;
        let at = marker_byte_offset(start).get();
        let prefix = &input.text()[input.byte_line_starts()[line]..at];
        let prefix = prefix.strip_prefix('\u{feff}').unwrap_or(prefix);
        let inline = prefix
            .trim_end_matches([' ', '\t'])
            .strip_suffix('-')
            .map(str::len);
        let above = if inline.is_none() {
            line.checked_sub(1).and_then(|above| {
                let text = input
                    .line_content(above)
                    .trim_start_matches('\u{feff}')
                    .trim_end_matches([' ', '\t']);
                text.strip_suffix('-')
                    .map(|prefix| (prefix.len(), RetainedPrefix::observe(prefix)))
            })
        } else {
            None
        };
        Self { inline, above }
    }

    pub(crate) const fn inline(&self) -> Option<usize> {
        self.inline
    }

    pub(crate) const fn above(&self) -> Option<(usize, RetainedPrefix)> {
        self.above
    }
}

pub(crate) struct DashGap<'a> {
    line: Option<&'a VirtualLine<'a>>,
}

impl<'a> DashGap<'a> {
    pub(super) const fn observe(line: Option<&'a VirtualLine<'a>>) -> Self {
        Self { line }
    }

    pub(crate) fn ends_at(&self, column: usize) -> bool {
        self.line
            .is_some_and(|line| line.prefix + line.body.trim_end().len() == column)
    }
}
