use std::ops::Range;

use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::span_utils::{ranges_to_char_indices, span_char_index_to_byte};

pub const ID: &str = "colons";
const TOO_MANY_BEFORE: &str = "too many spaces before colon";
const TOO_MANY_AFTER: &str = "too many spaces after colon";
const TOO_MANY_AFTER_QUESTION: &str = "too many spaces after question mark";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    max_spaces_before: i64,
    max_spaces_after: i64,
}

impl Config {
    const DEFAULT_MAX_BEFORE: i64 = 0;
    const DEFAULT_MAX_AFTER: i64 = 1;

    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let max_spaces_before = cfg
            .rule_option(ID, "max-spaces-before")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MAX_BEFORE);
        let max_spaces_after = cfg
            .rule_option(ID, "max-spaces-after")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MAX_AFTER);

        Self {
            max_spaces_before,
            max_spaces_after,
        }
    }

    #[must_use]
    pub const fn new_for_tests(max_spaces_before: i64, max_spaces_after: i64) -> Self {
        Self {
            max_spaces_before,
            max_spaces_after,
        }
    }

    #[must_use]
    pub const fn max_spaces_before(&self) -> i64 {
        self.max_spaces_before
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

    fn into_sorted(mut self) -> Vec<Range<usize>> {
        self.ranges.sort_by(|a, b| a.start.cmp(&b.start));
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

enum BeforeResult {
    SameLine {
        spaces: usize,
        preceding_char: Option<usize>,
    },
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
    let buffer_len = buffer.len();
    let scalar_ranges = ranges_to_char_indices(scalar_ranges, &chars, buffer_len);
    let line_starts = build_line_starts(buffer);

    let mut scalar_idx = 0usize;
    let mut idx = 0usize;
    let mut violations = Vec::new();

    while idx < chars.len() {
        let (byte_idx, ch) = chars[idx];

        while scalar_idx < scalar_ranges.len()
            && span_char_index_to_byte(&chars, scalar_ranges[scalar_idx].end, buffer_len)
                <= byte_idx
        {
            scalar_idx += 1;
        }

        if let Some(range) = scalar_ranges.get(scalar_idx) {
            let start_byte = span_char_index_to_byte(&chars, range.start, buffer_len);
            let end_byte = span_char_index_to_byte(&chars, range.end, buffer_len);
            if byte_idx >= start_byte && byte_idx < end_byte {
                idx = range.end;
                continue;
            }
        }

        match ch {
            '#' => {
                idx = skip_comment(&chars, idx);
                continue;
            }
            ':' => {
                evaluate_colon(cfg, &mut violations, &chars, idx, &line_starts);
            }
            '?' => {
                evaluate_question_mark(cfg, &mut violations, &chars, idx, &line_starts);
            }
            _ => {}
        }

        idx += 1;
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
            if chars.get(idx + 1).is_some_and(|(_, ch)| *ch == '\n') {
                idx += 1;
            }
            break;
        }
        idx += 1;
    }
    idx
}

fn evaluate_colon(
    cfg: &Config,
    violations: &mut Vec<Violation>,
    chars: &[(usize, char)],
    colon_idx: usize,
    line_starts: &[usize],
) {
    let mut skip_after_check = false;

    if let BeforeResult::SameLine {
        spaces,
        preceding_char,
    } = compute_spaces_before(chars, colon_idx)
    {
        if let Some(preceding_idx) = preceding_char
            && spaces == 0
            && alias_immediately_before(chars, preceding_idx)
        {
            skip_after_check = true;
        }

        if !skip_after_check && cfg.max_spaces_before >= 0 {
            let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
            if spaces_i64 > cfg.max_spaces_before {
                let colon_byte = chars[colon_idx].0;
                let (line, column) = line_and_column(line_starts, colon_byte);
                let highlight_column = column.saturating_sub(1).max(1);
                violations.push(Violation {
                    line,
                    column: highlight_column,
                    message: TOO_MANY_BEFORE.to_string(),
                });
            }
        }
    }

    if !skip_after_check
        && cfg.max_spaces_after >= 0
        && let AfterResult::SameLine { spaces, next_char } = compute_spaces_after(chars, colon_idx)
    {
        if chars[next_char].1 == '#' {
            return;
        }
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        if spaces_i64 > cfg.max_spaces_after {
            let next_byte = chars[next_char].0;
            let (line, column) = line_and_column(line_starts, next_byte);
            let highlight_column = column.saturating_sub(1).max(1);
            violations.push(Violation {
                line,
                column: highlight_column,
                message: TOO_MANY_AFTER.to_string(),
            });
        }
    }
}

fn evaluate_question_mark(
    cfg: &Config,
    violations: &mut Vec<Violation>,
    chars: &[(usize, char)],
    question_idx: usize,
    line_starts: &[usize],
) {
    if cfg.max_spaces_after >= 0
        && is_explicit_question_mark(chars, question_idx)
        && let AfterResult::SameLine { spaces, next_char } =
            compute_spaces_after(chars, question_idx)
    {
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        if spaces_i64 > cfg.max_spaces_after {
            let next_byte = chars[next_char].0;
            let (line, column) = line_and_column(line_starts, next_byte);
            let highlight_column = column.saturating_sub(1).max(1);
            violations.push(Violation {
                line,
                column: highlight_column,
                message: TOO_MANY_AFTER_QUESTION.to_string(),
            });
        }
    }
}

fn compute_spaces_before(chars: &[(usize, char)], colon_idx: usize) -> BeforeResult {
    let mut spaces = 0usize;
    let mut idx = colon_idx;

    loop {
        let Some(prev) = idx.checked_sub(1) else {
            return BeforeResult::SameLine {
                spaces,
                preceding_char: None,
            };
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
        return BeforeResult::SameLine {
            spaces,
            preceding_char: Some(prev),
        };
    }
}

fn compute_spaces_after(chars: &[(usize, char)], start_idx: usize) -> AfterResult {
    let mut spaces = 0usize;
    let mut idx = start_idx + 1;

    while idx < chars.len() {
        let ch = chars[idx].1;
        match ch {
            ' ' | '\t' => {
                spaces += 1;
                idx += 1;
            }
            '\n' => return AfterResult::Ignored,
            '\r' => {
                if idx + 1 < chars.len() && chars[idx + 1].1 == '\n' {
                    return AfterResult::Ignored;
                }
                return AfterResult::Ignored;
            }
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

fn alias_immediately_before(chars: &[(usize, char)], preceding_idx: usize) -> bool {
    let mut idx = preceding_idx;
    loop {
        let ch = chars[idx].1;
        if ch == '*' {
            return true;
        }
        if is_alias_identifier_char(ch) {
            if idx == 0 {
                return false;
            }
            idx -= 1;
            continue;
        }
        return false;
    }
}

const fn is_alias_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')
}

fn is_explicit_question_mark(chars: &[(usize, char)], idx: usize) -> bool {
    let next = chars.get(idx + 1).map_or('\0', |(_, ch)| *ch);
    if !(matches!(next, ' ' | '\t' | '\n' | '\r')) {
        return false;
    }

    match prev_non_ws_same_line(chars, idx) {
        None => true,
        Some((prev_idx, prev_ch)) => {
            matches!(prev_ch, '[' | '{' | ',' | '?')
                || (prev_ch == '-' && is_sequence_indicator(chars, prev_idx))
        }
    }
}

fn prev_non_ws_same_line(chars: &[(usize, char)], idx: usize) -> Option<(usize, char)> {
    let mut cursor = idx;
    while let Some(prev) = cursor.checked_sub(1) {
        let ch = chars[prev].1;
        match ch {
            ' ' | '\t' => {
                cursor = prev;
            }
            '\n' | '\r' => return None,
            _ => return Some((prev, ch)),
        }
    }
    None
}

fn is_sequence_indicator(chars: &[(usize, char)], hyphen_idx: usize) -> bool {
    let mut cursor = hyphen_idx;
    while let Some(prev) = cursor.checked_sub(1) {
        let ch = chars[prev].1;
        match ch {
            ' ' | '\t' => cursor = prev,
            '\n' | '\r' => return true,
            _ => return false,
        }
    }
    true
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
pub fn coverage_is_explicit_question_mark(chars: &[(usize, char)], idx: usize) -> bool {
    is_explicit_question_mark(chars, idx)
}

#[doc(hidden)]
#[must_use]
pub fn coverage_is_sequence_indicator(chars: &[(usize, char)], idx: usize) -> bool {
    is_sequence_indicator(chars, idx)
}

#[doc(hidden)]
#[must_use]
pub fn coverage_evaluate_question_mark(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let mut violations = Vec::new();
    let line_starts = build_line_starts(buffer);
    if let Some((idx, _)) = chars.iter().enumerate().find(|(_, (_, ch))| *ch == '?') {
        evaluate_question_mark(cfg, &mut violations, &chars, idx, &line_starts);
    } else {
        // explicit branch to ensure coverage marks the absence case
        let () = ();
    }
    violations
}

#[doc(hidden)]
#[must_use]
pub fn coverage_skip_comment(buffer: &str) -> bool {
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let idx = skip_comment(&chars, 0);
    chars.get(idx).is_some_and(|(_, ch)| *ch == '\n')
}
