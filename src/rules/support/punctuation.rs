use std::ops::Range;

use crate::rules::support::span_utils::{BytePos, CharPos, marker_byte_offset};
use crate::text_version::{Analysis, TextVersion};

/// Clamps `current` into `min..=max`; leaves it alone when `min > max`, since no spacing passes.
pub(crate) fn target_spacing(current: usize, min: i64, max: i64) -> usize {
    let min = usize::try_from(min).unwrap_or(0);
    let max = usize::try_from(max).unwrap_or(usize::MAX);
    if min > max {
        current
    } else {
        current.clamp(min, max)
    }
}

pub(crate) fn collect_scalar_ranges(buffer: &str) -> Vec<Range<BytePos>> {
    byte_scalar_ranges(TextVersion::new(buffer).analysis())
}

pub(crate) fn byte_scalar_ranges(input: Analysis<'_>) -> Vec<Range<BytePos>> {
    let mut ranges: Vec<_> = input
        .parsed()
        .scalars
        .iter()
        .filter_map(|(_, span)| {
            let start = marker_byte_offset(span.start);
            let end = marker_byte_offset(span.end);
            (start < end).then_some(start..end)
        })
        .collect();
    ranges.sort_by_key(|range| range.start);
    ranges
}

pub(crate) fn skip_comment(bytes: &[u8], mut idx: usize) -> usize {
    idx += 1;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\n' => break,
            b'\r' => {
                idx += usize::from(bytes.get(idx + 1) == Some(&b'\n'));
                break;
            }
            _ => idx += 1,
        }
    }
    idx
}

/// `CharPos` (not byte offset) at which each line begins, so derived columns are
/// 1-indexed character counts that match yamllint on multibyte lines. Reuses the
/// caller's `char_indices()` slice rather than decoding again.
pub(crate) fn build_line_starts(chars: &[(usize, char)]) -> Vec<CharPos> {
    let mut starts = vec![CharPos::new(0)];
    let mut idx = 0usize;
    while idx < chars.len() {
        match chars[idx].1 {
            '\n' => {
                starts.push(CharPos::new(idx + 1));
                idx += 1;
            }
            '\r' => {
                if chars.get(idx + 1).is_some_and(|(_, ch)| *ch == '\n') {
                    starts.push(CharPos::new(idx + 2));
                    idx += 2;
                } else {
                    starts.push(CharPos::new(idx + 1));
                    idx += 1;
                }
            }
            _ => idx += 1,
        }
    }

    starts
}

pub(crate) fn build_byte_line_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\r' => {
                idx += usize::from(bytes.get(idx + 1) == Some(&b'\n'));
                starts.push(idx + 1);
            }
            b'\n' => starts.push(idx + 1),
            _ => {}
        }
        idx += 1;
    }
    starts
}

pub(crate) fn byte_line_and_column(
    text: &[u8],
    starts: &[usize],
    byte: usize,
) -> (usize, usize) {
    let line = starts
        .partition_point(|&start| start <= byte)
        .saturating_sub(1);
    let prefix = &text[starts[line]..byte];
    let column = prefix.iter().filter(|&&byte| byte & 0xc0 != 0x80).count();
    (line + 1, column + 1)
}

/// A `CharPos` as a 1-indexed `(line, column)`. The `CharPos` parameter (not a raw
/// `usize`) makes passing a byte offset a compile error, so the column counts chars.
pub(crate) fn line_and_column(
    line_starts: &[CharPos],
    char_idx: CharPos,
) -> (usize, usize) {
    let mut left = 0usize;
    let mut right = line_starts.len();

    while left + 1 < right {
        let mid = usize::midpoint(left, right);
        if line_starts[mid] <= char_idx {
            left = mid;
        } else {
            right = mid;
        }
    }

    let line_start = line_starts[left];
    (
        left + 1,
        char_idx.get().saturating_sub(line_start.get()) + 1,
    )
}

pub(crate) fn template_double_curly_end(chars: &[u8], idx: usize) -> Option<usize> {
    if idx + 1 >= chars.len() || chars[idx] != b'{' || chars[idx + 1] != b'{' {
        return None;
    }
    let mut cursor = idx + 2;
    while cursor + 1 < chars.len() {
        if chars[cursor] == b'}' && chars[cursor + 1] == b'}' {
            let inner_contains_mapping = chars[idx + 2..cursor].contains(&b':');
            return (!inner_contains_mapping).then_some(cursor + 2);
        }
        cursor += 1;
    }
    let inner_contains_mapping = chars[idx + 2..].contains(&b':');
    (!inner_contains_mapping).then_some(chars.len())
}
