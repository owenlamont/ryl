use std::ops::Range;

use granit_parser::Marker;

use crate::rules::support::line_syntax::line_contents;

/// A byte offset into a UTF-8 buffer, valid for `&str` slicing. Distinct type from
/// [`CharPos`] so a character index can never be silently used as a byte offset.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct BytePos(usize);

/// A character index, as reported by granit spans via `Marker::index`; never used to
/// address bytes directly.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct CharPos(usize);

impl BytePos {
    #[must_use]
    pub const fn new(offset: usize) -> Self {
        Self(offset)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl CharPos {
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

#[must_use]
pub fn marker_byte_offset(marker: Marker) -> BytePos {
    BytePos(
        marker
            .byte_offset()
            .expect("granit Parser::new_from_str always populates byte offsets"),
    )
}

#[must_use]
pub fn byte_slice(buffer: &str, range: Range<BytePos>) -> &str {
    &buffer[range.start.0..range.end.0]
}

/// The scalar range containing char index `idx`, if any, advancing `cursor` past
/// ranges ending at or before it. Flow-rule scanners use this to skip scalar
/// interiors; `cursor` persists across calls for one left-to-right scan.
#[must_use]
pub fn containing_scalar_range<'a>(
    ranges: &'a [Range<CharPos>],
    cursor: &mut usize,
    idx: usize,
) -> Option<&'a Range<CharPos>> {
    while ranges
        .get(*cursor)
        .is_some_and(|range| range.end.get() <= idx)
    {
        *cursor += 1;
    }
    ranges
        .get(*cursor)
        .filter(|range| idx >= range.start.get() && idx < range.end.get())
}

/// Clamp a 1-based `(line, column)` onto a real position within `buffer`.
///
/// granit reports an implicit empty scalar (a node after a tag/anchor with no written
/// value) at a virtual position that, when such a node ends the document, can fall
/// past end-of-line. `tags`/`empty-values` clamp here so a diagnostic never points
/// outside the document.
#[must_use]
pub fn clamp_position(buffer: &str, line: usize, column: usize) -> (usize, usize) {
    // `line_contents` yields only real lines (no trailing-break phantom), so its
    // length is the last line a clamp may land on; CR-aware like granit's `Marker::line`.
    let lines = line_contents(buffer);
    let line = line.min(lines.len());
    let index = line
        .checked_sub(1)
        .expect("clamp_position receives a 1-based granit line number");
    let max_column = lines
        .get(index)
        .expect("clamped line always indexes the line table")
        .chars()
        .count()
        + 1;
    (line, column.min(max_column))
}

#[must_use]
pub fn apply_replacements(
    buffer: &str,
    mut replacements: Vec<(BytePos, BytePos, String)>,
) -> String {
    replacements.sort_by_key(|(start, _, _)| std::cmp::Reverse(start.0));
    let mut output = buffer.to_owned();
    for (start, end, text) in replacements {
        output.replace_range(start.0..end.0, &text);
    }
    output
}
