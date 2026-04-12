use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{
    build_line_starts, collect_scalar_ranges, line_and_column, skip_comment,
};
use crate::rules::support::span_utils::span_char_index_to_byte;

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
        Self {
            max_spaces_before: cfg.rule_option_int(
                ID,
                "max-spaces-before",
                Self::DEFAULT_MAX_BEFORE,
            ),
            min_spaces_after: cfg.rule_option_int(
                ID,
                "min-spaces-after",
                Self::DEFAULT_MIN_AFTER,
            ),
            max_spaces_after: cfg.rule_option_int(
                ID,
                "max-spaces-after",
                Self::DEFAULT_MAX_AFTER,
            ),
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

enum BeforeResult {
    SameLine { spaces: usize, start_idx: usize },
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

    let scalar_ranges = collect_scalar_ranges(buffer);
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let buffer_len = buffer.len();
    let line_starts = build_line_starts(buffer);

    let mut violations = Vec::new();
    let mut contexts: Vec<FlowKind> = Vec::new();
    let mut i = 0usize;
    let mut range_idx = 0usize;

    while i < chars.len() {
        let (byte_idx, ch) = chars[i];

        while range_idx < scalar_ranges.len()
            && span_char_index_to_byte(&chars, scalar_ranges[range_idx].end, buffer_len)
                <= byte_idx
        {
            range_idx += 1;
        }

        if let Some(range) = scalar_ranges.get(range_idx) {
            let start_byte = span_char_index_to_byte(&chars, range.start, buffer_len);
            let end_byte = span_char_index_to_byte(&chars, range.end, buffer_len);
            if byte_idx >= start_byte && byte_idx < end_byte {
                i = range.end;
                continue;
            }
        }

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

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    if buffer.is_empty() {
        return None;
    }

    let scalar_ranges = collect_scalar_ranges(buffer);
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let buffer_len = buffer.len();

    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    let mut contexts: Vec<FlowKind> = Vec::new();
    let mut i = 0usize;
    let mut range_idx = 0usize;

    while i < chars.len() {
        let (byte_idx, ch) = chars[i];

        while range_idx < scalar_ranges.len()
            && span_char_index_to_byte(&chars, scalar_ranges[range_idx].end, buffer_len)
                <= byte_idx
        {
            range_idx += 1;
        }

        if let Some(range) = scalar_ranges.get(range_idx) {
            let start_byte = span_char_index_to_byte(&chars, range.start, buffer_len);
            let end_byte = span_char_index_to_byte(&chars, range.end, buffer_len);
            if byte_idx >= start_byte && byte_idx < end_byte {
                i = range.end;
                continue;
            }
        }

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
            ',' if !contexts.is_empty() => {
                collect_comma_fixes(cfg, &chars, i, &mut replacements);
            }
            _ => {}
        }

        i += 1;
    }

    apply_replacements(buffer, replacements)
}

fn evaluate_comma(
    cfg: &Config,
    violations: &mut Vec<Violation>,
    chars: &[(usize, char)],
    comma_idx: usize,
    line_starts: &[usize],
) {
    if let BeforeResult::SameLine { spaces, .. } =
        compute_spaces_before(chars, comma_idx)
        && cfg.max_spaces_before >= 0
    {
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        if spaces_i64 > cfg.max_spaces_before {
            let comma_byte = chars[comma_idx].0;
            let (line, column) = line_and_column(line_starts, comma_byte);
            let highlight_column = column.saturating_sub(1).max(1);
            violations.push(Violation {
                line,
                column: highlight_column,
                message: TOO_MANY_BEFORE.to_string(),
            });
        }
    }

    if let AfterResult::SameLine { spaces, next_char } =
        compute_spaces_after(chars, comma_idx)
    {
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
            return BeforeResult::SameLine {
                spaces,
                start_idx: comma_idx,
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
            start_idx: idx,
        };
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

#[doc(hidden)]
#[must_use]
pub fn coverage_compute_spaces_before(buffer: &str, comma_idx: usize) -> Option<usize> {
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    debug_assert!(comma_idx < chars.len());
    match compute_spaces_before(&chars, comma_idx) {
        BeforeResult::SameLine { spaces, .. } => Some(spaces),
        BeforeResult::Ignored => None,
    }
}

#[doc(hidden)]
#[must_use]
pub fn coverage_skip_zero_length_span() -> usize {
    collect_scalar_ranges("").len()
}

fn collect_comma_fixes(
    cfg: &Config,
    chars: &[(usize, char)],
    comma_idx: usize,
    replacements: &mut Vec<(usize, usize, String)>,
) {
    if let BeforeResult::SameLine { spaces, start_idx } =
        compute_spaces_before(chars, comma_idx)
        && cfg.max_spaces_before >= 0
    {
        let target = usize::try_from(cfg.max_spaces_before).unwrap_or(usize::MAX);
        if spaces > target {
            replacements.push((
                chars[start_idx].0,
                chars[comma_idx].0,
                " ".repeat(target),
            ));
        }
    }

    if let AfterResult::SameLine { spaces, next_char } =
        compute_spaces_after(chars, comma_idx)
        && let Some(target) = target_spaces_after(cfg, spaces)
        && target != spaces
    {
        replacements.push((
            chars[comma_idx].0 + chars[comma_idx].1.len_utf8(),
            chars[next_char].0,
            " ".repeat(target),
        ));
    }
}

fn target_spaces_after(cfg: &Config, current: usize) -> Option<usize> {
    let min_spaces = usize::try_from(cfg.min_spaces_after).ok().unwrap_or(0);
    let max_spaces = if cfg.max_spaces_after >= 0 {
        usize::try_from(cfg.max_spaces_after).unwrap_or(usize::MAX)
    } else {
        usize::MAX
    };
    let target = current.max(min_spaces).min(max_spaces);
    (target != current).then_some(target)
}

fn apply_replacements(
    buffer: &str,
    mut replacements: Vec<(usize, usize, String)>,
) -> Option<String> {
    if replacements.is_empty() {
        return None;
    }

    replacements.sort_by(|left, right| right.0.cmp(&left.0));

    let mut output = buffer.to_string();
    for (start, end, replacement) in replacements {
        output.replace_range(start..end, &replacement);
    }
    Some(output)
}

#[doc(hidden)]
#[must_use]
pub fn coverage_skip_comment_crlf() -> (usize, usize) {
    let chars_crlf: Vec<(usize, char)> = "#\r\n".char_indices().collect();
    let idx_crlf = skip_comment(&chars_crlf, 0);

    let chars_cr: Vec<(usize, char)> = "#\r".char_indices().collect();
    let idx_cr = skip_comment(&chars_cr, 0);

    (idx_crlf, idx_cr)
}
