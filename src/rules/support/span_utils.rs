use ryl_yaml_parser::Marker;

/// A byte offset into a UTF-8 buffer, valid for `&str` slicing. Distinct type from
/// `CharPos` so a character index can never be silently used as a byte offset.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct BytePos(usize);

/// A character index, as reported by parser spans via `Marker::index`; never used to
/// address bytes directly.
#[cfg(feature = "lsp")]
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

#[cfg(feature = "lsp")]
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
            .expect("Parser::new_from_str always populates byte offsets"),
    )
}

#[must_use]
pub fn apply_replacements(
    buffer: &str,
    mut replacements: Vec<(BytePos, BytePos, String)>,
) -> String {
    replacements.sort_by_key(|(start, _, _)| std::cmp::Reverse(start.0));
    if replacements
        .windows(2)
        .all(|pair| pair[1].1 <= pair[0].0 && pair[1].0 < pair[0].0)
    {
        let mut output = String::with_capacity(buffer.len());
        let mut cursor = 0;
        for (start, end, text) in replacements.into_iter().rev() {
            output.push_str(&buffer[cursor..start.0]);
            output.push_str(&text);
            cursor = end.0;
        }
        output.push_str(&buffer[cursor..]);
        return output;
    }
    let mut output = buffer.to_owned();
    for (start, end, text) in replacements {
        output.replace_range(start.0..end.0, &text);
    }
    output
}
