use std::ops::Range;

pub fn ranges_to_char_indices(
    ranges: Vec<Range<usize>>,
    chars: &[(usize, char)],
    buffer_len: usize,
) -> Vec<Range<usize>> {
    ranges
        .into_iter()
        .map(|range| {
            let start = byte_index_to_char(chars, range.start, buffer_len);
            let end = byte_index_to_char(chars, range.end, buffer_len);
            start..end
        })
        .collect()
}

pub fn span_char_index_to_byte(
    chars: &[(usize, char)],
    char_idx: usize,
    buffer_len: usize,
) -> usize {
    if char_idx >= chars.len() {
        buffer_len
    } else {
        chars[char_idx].0
    }
}

fn byte_index_to_char(chars: &[(usize, char)], byte_idx: usize, buffer_len: usize) -> usize {
    let clamped = byte_idx.min(buffer_len);
    chars.partition_point(|(offset, _)| *offset < clamped)
}
