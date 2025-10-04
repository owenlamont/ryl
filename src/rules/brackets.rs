use std::ops::Range;

use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::span_utils::ranges_to_char_indices;

pub const ID: &str = "brackets";

const FORBIDDEN_FLOW_SEQUENCE: &str = "forbidden flow sequence";
const TOO_FEW_SPACES_INSIDE: &str = "too few spaces inside brackets";
const TOO_MANY_SPACES_INSIDE: &str = "too many spaces inside brackets";
const TOO_FEW_SPACES_INSIDE_EMPTY: &str = "too few spaces inside empty brackets";
const TOO_MANY_SPACES_INSIDE_EMPTY: &str = "too many spaces inside empty brackets";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Forbid {
    None,
    All,
    NonEmpty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    forbid: Forbid,
    min_spaces_inside: i64,
    max_spaces_inside: i64,
    min_spaces_inside_empty: i64,
    max_spaces_inside_empty: i64,
}

impl Config {
    const DEFAULT_MIN_SPACES_INSIDE: i64 = 0;
    const DEFAULT_MAX_SPACES_INSIDE: i64 = 0;
    const DEFAULT_MIN_SPACES_INSIDE_EMPTY: i64 = -1;
    const DEFAULT_MAX_SPACES_INSIDE_EMPTY: i64 = -1;

    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let forbid = cfg.rule_option(ID, "forbid").map_or(Forbid::None, |node| {
            match (node.as_bool(), node.as_str()) {
                (Some(true), _) => Forbid::All,
                (None, Some("non-empty")) => Forbid::NonEmpty,
                _ => Forbid::None,
            }
        });

        let min_spaces_inside = cfg
            .rule_option(ID, "min-spaces-inside")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MIN_SPACES_INSIDE);
        let max_spaces_inside = cfg
            .rule_option(ID, "max-spaces-inside")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MAX_SPACES_INSIDE);
        let min_spaces_inside_empty = cfg
            .rule_option(ID, "min-spaces-inside-empty")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MIN_SPACES_INSIDE_EMPTY);
        let max_spaces_inside_empty = cfg
            .rule_option(ID, "max-spaces-inside-empty")
            .and_then(saphyr::YamlOwned::as_integer)
            .unwrap_or(Self::DEFAULT_MAX_SPACES_INSIDE_EMPTY);

        Self {
            forbid,
            min_spaces_inside,
            max_spaces_inside,
            min_spaces_inside_empty,
            max_spaces_inside_empty,
        }
    }

    #[must_use]
    pub const fn new_for_tests(
        forbid: Forbid,
        min_spaces_inside: i64,
        max_spaces_inside: i64,
        min_spaces_inside_empty: i64,
        max_spaces_inside_empty: i64,
    ) -> Self {
        Self {
            forbid,
            min_spaces_inside,
            max_spaces_inside,
            min_spaces_inside_empty,
            max_spaces_inside_empty,
        }
    }

    #[must_use]
    pub const fn effective_min_empty(&self) -> i64 {
        if self.min_spaces_inside_empty >= 0 {
            self.min_spaces_inside_empty
        } else {
            self.min_spaces_inside
        }
    }

    #[must_use]
    pub const fn effective_max_empty(&self) -> i64 {
        if self.max_spaces_inside_empty >= 0 {
            self.max_spaces_inside_empty
        } else {
            self.max_spaces_inside
        }
    }

    #[must_use]
    pub const fn forbid(&self) -> Forbid {
        self.forbid
    }

    #[must_use]
    pub const fn min_spaces_inside(&self) -> i64 {
        self.min_spaces_inside
    }

    #[must_use]
    pub const fn max_spaces_inside(&self) -> i64 {
        self.max_spaces_inside
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

#[derive(Debug, Clone, Copy)]
struct SequenceState {
    is_empty: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum AfterResult {
    SameLine { spaces: usize, next_idx: usize },
    Ignored,
}

#[derive(Debug, PartialEq, Eq)]
enum BeforeResult {
    SameLine { spaces: usize },
    Empty,
    Ignored,
}

#[derive(Clone, Copy)]
struct SpacingMessages<'a> {
    min: &'a str,
    max: &'a str,
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

    let mut range_idx = 0usize;
    let mut idx = 0usize;
    let mut stack: Vec<SequenceState> = Vec::new();
    let mut violations = Vec::new();

    while idx < chars.len() {
        while range_idx < scalar_ranges.len() && scalar_ranges[range_idx].end <= idx {
            range_idx += 1;
        }

        if let Some(range) = scalar_ranges.get(range_idx)
            && idx >= range.start
            && idx < range.end
        {
            if idx == range.start
                && let Some(state) = stack.last_mut()
            {
                state.is_empty = false;
            }
            idx = range.end;
            continue;
        }

        let ch = chars[idx].1;
        match ch {
            '[' => {
                if let Some(state) = stack.last_mut() {
                    state.is_empty = false;
                }
                handle_open(cfg, &chars, idx, &line_starts, &mut stack, &mut violations);
            }
            ']' => {
                handle_close(cfg, &chars, idx, &line_starts, &mut stack, &mut violations);
            }
            '#' => {
                idx = skip_comment(&chars, idx);
                continue;
            }
            ',' | ' ' | '\t' | '\n' => {}
            '\r' => {
                if idx + 1 < chars.len() && chars[idx + 1].1 == '\n' {
                    idx += 1;
                }
            }
            _ => {
                if let Some(state) = stack.last_mut() {
                    state.is_empty = false;
                }
            }
        }

        idx += 1;
    }

    violations
}

fn handle_open(
    cfg: &Config,
    chars: &[(usize, char)],
    idx: usize,
    line_starts: &[usize],
    stack: &mut Vec<SequenceState>,
    violations: &mut Vec<Violation>,
) {
    let open_byte = chars[idx].0;
    let (line, column) = line_and_column(line_starts, open_byte);
    let next_significant = next_significant_index(chars, idx);

    let mut skip_open_check = false;
    match cfg.forbid() {
        Forbid::All => {
            violations.push(Violation {
                line,
                column: column + 1,
                message: FORBIDDEN_FLOW_SEQUENCE.to_string(),
            });
            skip_open_check = true;
        }
        Forbid::NonEmpty => {
            let is_empty = matches!(next_significant.map(|j| chars[j].1), Some(']'));
            if !is_empty {
                violations.push(Violation {
                    line,
                    column: column + 1,
                    message: FORBIDDEN_FLOW_SEQUENCE.to_string(),
                });
                skip_open_check = true;
            }
        }
        Forbid::None => {}
    }

    let mut state = SequenceState {
        is_empty: matches!(next_significant.map(|j| chars[j].1), Some(']')),
    };

    if !skip_open_check
        && let AfterResult::SameLine { spaces, next_idx } = compute_spaces_after_open(chars, idx)
    {
        let next_byte = chars[next_idx].0;
        let (line, next_column) = line_and_column(line_starts, next_byte);
        if state.is_empty && chars[next_idx].1 == ']' {
            record_after_spacing(
                cfg.effective_min_empty(),
                cfg.effective_max_empty(),
                spaces,
                line,
                next_column,
                SpacingMessages {
                    min: TOO_FEW_SPACES_INSIDE_EMPTY,
                    max: TOO_MANY_SPACES_INSIDE_EMPTY,
                },
                violations,
            );
        } else {
            state.is_empty = false;
            record_after_spacing(
                cfg.min_spaces_inside(),
                cfg.max_spaces_inside(),
                spaces,
                line,
                next_column,
                SpacingMessages {
                    min: TOO_FEW_SPACES_INSIDE,
                    max: TOO_MANY_SPACES_INSIDE,
                },
                violations,
            );
        }
    }

    stack.push(state);
}

fn handle_close(
    cfg: &Config,
    chars: &[(usize, char)],
    idx: usize,
    line_starts: &[usize],
    stack: &mut Vec<SequenceState>,
    violations: &mut Vec<Violation>,
) {
    let Some(state) = stack.pop() else {
        return;
    };

    if state.is_empty {
        return;
    }

    match compute_spaces_before_close(chars, idx) {
        BeforeResult::SameLine { spaces } => {
            let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
            let bracket_byte = chars[idx].0;
            let (line, bracket_column) = line_and_column(line_starts, bracket_byte);
            if cfg.max_spaces_inside() >= 0 && spaces_i64 > cfg.max_spaces_inside() {
                let highlight = bracket_column.saturating_sub(1).max(1);
                violations.push(Violation {
                    line,
                    column: highlight,
                    message: TOO_MANY_SPACES_INSIDE.to_string(),
                });
            }
            if cfg.min_spaces_inside() >= 0 && spaces_i64 < cfg.min_spaces_inside() {
                violations.push(Violation {
                    line,
                    column: bracket_column,
                    message: TOO_FEW_SPACES_INSIDE.to_string(),
                });
            }
        }
        BeforeResult::Empty | BeforeResult::Ignored => {}
    }
}

fn record_after_spacing(
    min: i64,
    max: i64,
    spaces: usize,
    line: usize,
    next_column: usize,
    messages: SpacingMessages<'_>,
    violations: &mut Vec<Violation>,
) {
    let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
    if max >= 0 && spaces_i64 > max {
        let highlight = next_column.saturating_sub(1).max(1);
        violations.push(Violation {
            line,
            column: highlight,
            message: messages.max.to_string(),
        });
    }
    if min >= 0 && spaces_i64 < min {
        violations.push(Violation {
            line,
            column: next_column,
            message: messages.min.to_string(),
        });
    }
}

fn compute_spaces_after_open(chars: &[(usize, char)], open_idx: usize) -> AfterResult {
    let mut spaces = 0usize;
    let mut idx = open_idx + 1;
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
                    next_idx: idx,
                };
            }
        }
    }
    AfterResult::Ignored
}

fn compute_spaces_before_close(chars: &[(usize, char)], close_idx: usize) -> BeforeResult {
    if close_idx == 0 {
        return BeforeResult::Ignored;
    }
    let mut spaces = 0usize;
    let mut idx = close_idx;
    while idx > 0 {
        idx -= 1;
        match chars[idx].1 {
            ' ' | '\t' => {
                spaces += 1;
            }
            '\n' | '\r' | '#' => return BeforeResult::Ignored,
            '[' => {
                return BeforeResult::Empty;
            }
            _ => {
                return BeforeResult::SameLine { spaces };
            }
        }
    }
    BeforeResult::Ignored
}

fn next_significant_index(chars: &[(usize, char)], open_idx: usize) -> Option<usize> {
    let mut idx = open_idx + 1;
    while idx < chars.len() {
        match chars[idx].1 {
            ' ' | '\t' | '\n' => idx += 1,
            '\r' => {
                if idx + 1 < chars.len() && chars[idx + 1].1 == '\n' {
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            '#' => {
                idx = skip_comment(chars, idx);
                if idx >= chars.len() {
                    continue;
                }
                idx += 1;
            }
            _ => return Some(idx),
        }
    }
    None
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
