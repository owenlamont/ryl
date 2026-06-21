use std::ops::Range;

use granit_parser::{
    Event, Parser, Scanner, Span, SpannedEventReceiver, StrInput, TokenType,
};

use crate::rules::support::span_utils::CharPos;

pub(crate) fn collect_scalar_ranges(buffer: &str) -> Vec<Range<CharPos>> {
    let mut parser = Parser::new_from_str(buffer);
    let mut collector = ScalarRangeCollector::new();
    let _ = parser.load(&mut collector, true);
    collector.into_sorted()
}

/// `CharPos` just past each alias token (`*name`), from the scanner so it is
/// independent of anchor resolution: an undefined or forward alias is still a token
/// (the parser would error on it). `colons` uses these to exempt the required space
/// before an alias mapping key (`*foo : bar`).
pub(crate) fn collect_alias_ends(buffer: &str) -> Vec<CharPos> {
    Scanner::new(StrInput::new(buffer))
        .filter(|token| matches!(token.1, TokenType::Alias(_)))
        .map(|token| CharPos::new(token.0.end.index()))
        .collect()
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
    (left + 1, char_idx.get() - line_start.get() + 1)
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
