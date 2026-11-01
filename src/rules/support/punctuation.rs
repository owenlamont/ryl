use std::cell::Cell;

use crate::rules::support::span_utils::BytePos;
#[cfg(feature = "lsp")]
use crate::rules::support::span_utils::CharPos;
use crate::text_version::{Analysis, lexical::Kind};

pub(crate) struct Gap {
    start: usize,
    end: usize,
    spaces: usize,
}

impl Gap {
    pub(crate) const fn new(start: usize, end: usize, spaces: usize) -> Self {
        Self { start, end, spaces }
    }

    pub(crate) const fn spaces(&self) -> usize {
        self.spaces
    }

    pub(crate) fn select(
        self,
        spaces: usize,
    ) -> Option<super::token_spacing::SpaceEdit> {
        (spaces != self.spaces).then_some((
            BytePos::new(self.start),
            BytePos::new(self.end),
            spaces,
        ))
    }
}

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

pub(crate) fn flow_sites(
    input: Analysis<'_>,
    skip_templates: bool,
) -> Vec<(usize, u8)> {
    let bytes = input.text().as_bytes();
    let mut sites = Vec::new();
    let mut depth = 0usize;
    let mut skip_to = 0;
    for (start, end, kind) in input.tokens().bytes() {
        if start >= end || start < skip_to {
            continue;
        }
        match kind {
            Kind::FlowMappingStart | Kind::FlowSequenceStart => {
                if skip_templates
                    && let Some(end) = template_double_curly_end(bytes, start)
                {
                    skip_to = end;
                    continue;
                }
                depth += 1;
            }
            Kind::FlowMappingEnd | Kind::FlowSequenceEnd => {
                depth = depth.saturating_sub(1);
            }
            Kind::FlowEntry => {}
            Kind::Comment { .. } => continue,
            _ => {
                if depth > 0 {
                    flow_content(&mut sites, start);
                }
                continue;
            }
        }
        sites.push((start, bytes[start]));
    }
    sites
}

fn flow_content(sites: &mut Vec<(usize, u8)>, idx: usize) {
    if sites.last().is_none_or(|&(_, ch)| ch != b'x') {
        sites.push((idx, b'x'));
    }
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
#[cfg(feature = "lsp")]
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

/// 1-based `(line, column)` of byte offsets, counting on from the previous lookup when it
/// sits earlier on the same line, so ascending lookups along one long line stay linear.
pub(crate) struct Columns<'a> {
    text: &'a [u8],
    starts: &'a [usize],
    last: Cell<(usize, usize)>,
}

impl<'a> Columns<'a> {
    pub(crate) const fn new(text: &'a [u8], starts: &'a [usize]) -> Self {
        Self {
            text,
            starts,
            last: Cell::new((0, 0)),
        }
    }

    pub(crate) fn at(&self, byte: usize) -> (usize, usize) {
        let line = self
            .starts
            .partition_point(|&start| start <= byte)
            .saturating_sub(1);
        let (last, chars) = self.last.get();
        let (from, counted) = if (self.starts[line]..=byte).contains(&last) {
            (last, chars)
        } else {
            (self.starts[line], 0)
        };
        let column = counted
            + self.text[from..byte]
                .iter()
                .filter(|&&byte| byte & 0xc0 != 0x80)
                .count();
        self.last.set((byte, column));
        (line + 1, column + 1)
    }
}

/// A `CharPos` as a 1-indexed `(line, column)`. The `CharPos` parameter (not a raw
/// `usize`) makes passing a byte offset a compile error, so the column counts chars.
#[cfg(feature = "lsp")]
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

fn template_double_curly_end(chars: &[u8], idx: usize) -> Option<usize> {
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AfterResult {
    SameLine { spaces: usize, next_idx: usize },
    Ignored,
}

/// The blanks after the byte at `idx`, or `Ignored` when a line end or comment follows.
pub(crate) fn compute_spaces_after(bytes: &[u8], idx: usize) -> AfterResult {
    let mut spaces = 0usize;
    let mut idx = idx + 1;
    loop {
        match bytes.get(idx) {
            Some(b' ' | b'\t') => {
                spaces += 1;
                idx += 1;
            }
            None | Some(b'\n' | b'\r' | b'#') => return AfterResult::Ignored,
            Some(_) => {
                return AfterResult::SameLine {
                    spaces,
                    next_idx: idx,
                };
            }
        }
    }
}
