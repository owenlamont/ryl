use std::ops::Range;

use granit_parser::Marker;

/// A byte offset into a UTF-8 buffer. Valid for `&str` slicing and
/// `String::replace_range`. Construct one only through the helpers here so a
/// character index can never be silently used as a byte offset (issue #232).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct BytePos(usize);

/// A character index, as reported by granit spans via `Marker::index`. Must be
/// converted with [`char_pos_to_byte`] before it can address bytes.
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
pub fn char_pos_to_byte(
    chars: &[(usize, char)],
    pos: CharPos,
    buffer_len: usize,
) -> BytePos {
    if pos.0 >= chars.len() {
        BytePos(buffer_len)
    } else {
        BytePos(chars[pos.0].0)
    }
}

#[must_use]
pub fn byte_slice(buffer: &str, range: Range<BytePos>) -> &str {
    &buffer[range.start.0..range.end.0]
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
