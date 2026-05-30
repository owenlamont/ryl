use std::ops::Range;

use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::rules::support::span_utils::CharPos;

pub(crate) fn collect_scalar_ranges(buffer: &str) -> Vec<Range<CharPos>> {
    let mut parser = Parser::new_from_str(buffer);
    let mut collector = ScalarRangeCollector::new();
    let _ = parser.load(&mut collector, true);
    collector.into_sorted()
}

pub(crate) fn skip_comment(chars: &[(usize, char)], mut idx: usize) -> usize {
    idx += 1;
    while idx < chars.len() {
        let ch = chars[idx].1;
        if ch == '\n' {
            break;
        }
        if ch == '\r' {
            if chars.get(idx + 1).is_some_and(|(_, ch)| *ch == '\n') {
                idx += 1;
            }
            break;
        }
        idx += 1;
    }
    idx
}

/// Character indices (not byte offsets) at which each line begins, so columns
/// derived from them are 1-indexed character counts that match yamllint on
/// multibyte lines (issue #232).
pub(crate) fn build_line_starts(buffer: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut chars = buffer.chars().peekable();
    let mut char_idx = 0usize;
    while let Some(ch) = chars.next() {
        char_idx += 1;
        match ch {
            '\n' => starts.push(char_idx),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                    char_idx += 1;
                }
                starts.push(char_idx);
            }
            _ => {}
        }
    }

    starts
}

/// Resolve a character index into a 1-indexed `(line, column)` pair. `char_idx`
/// must be a character index (e.g. an index into `char_indices()`), never a
/// byte offset, so the column counts characters rather than bytes (issue #232).
pub(crate) fn line_and_column(
    line_starts: &[usize],
    char_idx: usize,
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
    (left + 1, char_idx - line_start + 1)
}

pub(crate) fn template_double_curly_end(
    chars: &[(usize, char)],
    idx: usize,
) -> Option<usize> {
    if idx + 1 >= chars.len() || chars[idx].1 != '{' || chars[idx + 1].1 != '{' {
        return None;
    }
    let mut cursor = idx + 2;
    while cursor + 1 < chars.len() {
        if chars[cursor].1 == '}' && chars[cursor + 1].1 == '}' {
            let inner_contains_mapping =
                chars[idx + 2..cursor].iter().any(|(_, ch)| *ch == ':');
            return (!inner_contains_mapping).then_some(cursor + 2);
        }
        cursor += 1;
    }
    let inner_contains_mapping = chars[idx + 2..].iter().any(|(_, ch)| *ch == ':');
    (!inner_contains_mapping).then_some(chars.len())
}

struct ScalarRangeCollector {
    ranges: Vec<Range<CharPos>>,
}

impl ScalarRangeCollector {
    const fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    fn push_range(&mut self, span: Span) {
        let start = CharPos::new(span.start.index());
        let end = CharPos::new(span.end.index());
        if start < end {
            self.ranges.push(start..end);
        }
    }

    fn into_sorted(mut self) -> Vec<Range<CharPos>> {
        self.ranges.sort_by_key(|a| a.start);
        self.ranges
    }
}

impl SpannedEventReceiver<'_> for ScalarRangeCollector {
    fn on_event(&mut self, ev: Event<'_>, span: Span) {
        if matches!(ev, Event::Scalar(..)) {
            self.push_range(span);
        }
    }
}
