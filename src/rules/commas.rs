use std::ops::Range;

use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;

pub const ID: &str = "commas";
const TOO_MANY_BEFORE: &str = "too many spaces before comma";
const TOO_FEW_AFTER: &str = "too few spaces after comma";
const TOO_MANY_AFTER: &str = "too many spaces after comma";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    max_spaces_before: i64,
    min_spaces_after: i64,
    max_spaces_after: i64,
}

impl Config {
    const DEFAULT_MAX_BEFORE: i64 = 0;
    const DEFAULT_MIN_AFTER: i64 = 1;
    const DEFAULT_MAX_AFTER: i64 = 1;

    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let max_spaces_before = cfg
            .rule_option(ID, "max-spaces-before")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MAX_BEFORE);
        let min_spaces_after = cfg
            .rule_option(ID, "min-spaces-after")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MIN_AFTER);
        let max_spaces_after = cfg
            .rule_option(ID, "max-spaces-after")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MAX_AFTER);

        Self {
            max_spaces_before,
            min_spaces_after,
            max_spaces_after,
        }
    }

    #[must_use]
    pub const fn new_for_tests(
        max_spaces_before: i64,
        min_spaces_after: i64,
        max_spaces_after: i64,
    ) -> Self {
        Self {
            max_spaces_before,
            min_spaces_after,
            max_spaces_after,
        }
    }

    #[must_use]
    pub const fn max_spaces_before(&self) -> i64 {
        self.max_spaces_before
    }

    #[must_use]
    pub const fn min_spaces_after(&self) -> i64 {
        self.min_spaces_after
    }

    #[must_use]
    pub const fn max_spaces_after(&self) -> i64 {
        self.max_spaces_after
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

enum FlowKind {
    Sequence,
    Mapping,
}

struct ScalarRangeCollector {
    ranges: Vec<Range<usize>>,
}

impl ScalarRangeCollector {
    const fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    fn push_range(&mut self, span: Span) {
        let start = span.start.index();
        let end = span.end.index();
        if start < end {
            self.ranges.push(start..end);
        }
    }

    fn into_sorted(self) -> Vec<Range<usize>> {
        let mut ranges = self.ranges;
        ranges.sort_by(|a, b| a.start.cmp(&b.start));
        ranges
    }
}

impl SpannedEventReceiver<'_> for ScalarRangeCollector {
    fn on_event(&mut self, ev: Event<'_>, span: Span) {
        if matches!(ev, Event::Scalar(..)) {
            self.push_range(span);
        }
    }
}

enum BeforeResult {
    SameLine { spaces: usize },
    Ignored,
}

enum AfterResult {
    SameLine { spaces: usize, next_char: usize },
    Ignored,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    if buffer.is_empty() {
        return Vec::new();
    }

    let mut parser = Parser::new_from_str(buffer);
    let mut collector = ScalarRangeCollector::new();
    let _ = parser.load(&mut collector, true);
    let scalar_ranges = collector.into_sorted();

    let chars: Vec<(usize, char)> = buffer.char_indices().collect();

    let line_starts = build_line_starts(buffer);

    let mut violations = Vec::new();
    let mut contexts: Vec<FlowKind> = Vec::new();
    let mut i = 0usize;
    let mut range_idx = 0usize;

    while i < chars.len() {
        while range_idx < scalar_ranges.len() && scalar_ranges[range_idx].end <= i {
            range_idx += 1;
        }

        if let Some(range) = scalar_ranges.get(range_idx)
            && i >= range.start
            && i < range.end
        {
            i = range.end;
            continue;
        }

        let (_byte_idx, ch) = chars[i];

        match ch {
            '[' => contexts.push(FlowKind::Sequence),
            '{' => contexts.push(FlowKind::Mapping),
            ']' | '}' => {
                contexts.pop();
            }
            '#' => {
                i = skip_comment(&chars, i);
                continue;
            }
            ',' => {
                if !contexts.is_empty() {
                    evaluate_comma(cfg, &mut violations, &chars, i, &line_starts);
                }
            }
            _ => {}
        }

        i += 1;
    }

    violations
}

fn skip_comment(chars: &[(usize, char)], mut idx: usize) -> usize {
    idx += 1;
    while idx < chars.len() {
        let ch = chars[idx].1;
        if ch == '\n' {
            break;
        }
        if ch == '\r' {
            if idx + 1 < chars.len() && chars[idx + 1].1 == '\n' {
                idx += 1;
            }
            break;
        }
        idx += 1;
    }
    idx
}

fn evaluate_comma(
    cfg: &Config,
    violations: &mut Vec<Violation>,
    chars: &[(usize, char)],
    comma_idx: usize,
    line_starts: &[usize],
) {
    if let BeforeResult::SameLine { spaces } = compute_spaces_before(chars, comma_idx)
        && cfg.max_spaces_before >= 0
    {
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        if spaces_i64 > cfg.max_spaces_before {
            let comma_byte = chars[comma_idx].0;
            let (line, mut column) = line_and_column(line_starts, comma_byte);
            if column > 1 {
                column -= 1;
            }
            violations.push(Violation {
                line,
                column,
                message: TOO_MANY_BEFORE.to_string(),
            });
        }
    }

    if let AfterResult::SameLine { spaces, next_char } = compute_spaces_after(chars, comma_idx) {
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        let next_byte = chars[next_char].0;
        let (line, column) = line_and_column(line_starts, next_byte);
        if cfg.max_spaces_after >= 0 && spaces_i64 > cfg.max_spaces_after {
            let highlight_column = column.saturating_sub(1).max(1);
            violations.push(Violation {
                line,
                column: highlight_column,
                message: TOO_MANY_AFTER.to_string(),
            });
        }
        if cfg.min_spaces_after >= 0 && spaces_i64 < cfg.min_spaces_after {
            violations.push(Violation {
                line,
                column,
                message: TOO_FEW_AFTER.to_string(),
            });
        }
    }
}

fn compute_spaces_before(chars: &[(usize, char)], comma_idx: usize) -> BeforeResult {
    let mut spaces = 0usize;
    let mut idx = comma_idx;

    loop {
        let Some(prev) = idx.checked_sub(1) else {
            return BeforeResult::SameLine { spaces };
        };

        let ch = chars[prev].1;
        if matches!(ch, ' ' | '\t') {
            spaces += 1;
            idx = prev;
            continue;
        }
        if matches!(ch, '\n' | '\r') {
            return BeforeResult::Ignored;
        }
        return BeforeResult::SameLine { spaces };
    }
}

fn compute_spaces_after(chars: &[(usize, char)], comma_idx: usize) -> AfterResult {
    let mut spaces = 0usize;
    let mut idx = comma_idx + 1;
    while idx < chars.len() {
        match chars[idx].1 {
            ' ' | '\t' => {
                spaces += 1;
                idx += 1;
            }
            '\n' | '\r' | '#' => return AfterResult::Ignored,
            _ => {
                return AfterResult::SameLine {
                    spaces,
                    next_char: idx,
                };
            }
        }
    }
    AfterResult::Ignored
}

fn build_line_starts(buffer: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    starts.push(0);
    let bytes = buffer.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\n' => {
                starts.push(idx + 1);
                idx += 1;
            }
            b'\r' => {
                if idx + 1 < bytes.len() && bytes[idx + 1] == b'\n' {
                    starts.push(idx + 2);
                    idx += 2;
                } else {
                    starts.push(idx + 1);
                    idx += 1;
                }
            }
            _ => idx += 1,
        }
    }
    starts
}

fn line_and_column(line_starts: &[usize], byte_idx: usize) -> (usize, usize) {
    let mut left = 0usize;
    let mut right = line_starts.len();
    while left + 1 < right {
        let mid = usize::midpoint(left, right);
        if line_starts[mid] <= byte_idx {
            left = mid;
        } else {
            right = mid;
        }
    }
    let line_start = line_starts[left];
    (left + 1, byte_idx - line_start + 1)
}

#[doc(hidden)]
#[must_use]
pub fn coverage_compute_spaces_before(buffer: &str, comma_idx: usize) -> Option<usize> {
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    debug_assert!(comma_idx < chars.len());
    match compute_spaces_before(&chars, comma_idx) {
        BeforeResult::SameLine { spaces } => Some(spaces),
        BeforeResult::Ignored => None,
    }
}
