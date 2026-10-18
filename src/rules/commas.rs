//! `commas`: spacing around `,` separators in flow collections: no space before,
//! exactly one after. Mirrors yamllint's `commas`. Safe `--fix` normalises the spacing.

use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{
    collect_scalar_ranges, line_and_column, skip_comment, target_spacing,
    template_double_curly_end,
};
use crate::rules::support::span_utils::{
    BytePos, CharPos, apply_replacements, containing_scalar_range,
};
use crate::text_version::{Analysis, TextVersion};

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
    pub const fn new(
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
    check_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn check_with_analysis(input: Analysis<'_>, cfg: &Config) -> Vec<Violation> {
    let line_starts = input.line_starts();
    let mut violations = Vec::new();
    scan_commas(input, |chars, i| {
        evaluate_comma(cfg, &mut violations, chars, i, line_starts);
    });
    violations
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    fix_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>, cfg: &Config) -> Option<String> {
    let buffer = input.text();
    let mut replacements: Vec<(BytePos, BytePos, String)> = Vec::new();
    scan_commas(input, |chars, i| {
        collect_comma_fixes(cfg, chars, i, &mut replacements);
    });

    if replacements.is_empty() {
        return None;
    }
    Some(apply_replacements(buffer, replacements))
}

fn scan_commas(input: Analysis<'_>, mut visit: impl FnMut(&[(usize, char)], usize)) {
    let scalar_ranges = crate::rules::support::punctuation::scalar_ranges(input);
    let chars = input.chars();
    let mut contexts: Vec<FlowKind> = Vec::new();
    let mut i = 0usize;
    let mut range_idx = 0usize;

    while i < chars.len() {
        if let Some(range) = containing_scalar_range(&scalar_ranges, &mut range_idx, i)
        {
            i = range.end.get();
            continue;
        }

        match chars[i].1 {
            '[' => contexts.push(FlowKind::Sequence),
            '{' => {
                if let Some(next_idx) = template_double_curly_end(chars, i) {
                    i = next_idx;
                    continue;
                }
                contexts.push(FlowKind::Mapping);
            }
            ']' | '}' => {
                contexts.pop();
            }
            '#' => {
                i = skip_comment(chars, i);
                continue;
            }
            ',' if !contexts.is_empty() => {
                visit(chars, i);
            }
            _ => {}
        }

        i += 1;
    }
}

fn evaluate_comma(
    cfg: &Config,
    violations: &mut Vec<Violation>,
    chars: &[(usize, char)],
    comma_idx: usize,
    line_starts: &[CharPos],
) {
    if let BeforeResult::SameLine { spaces, .. } =
        compute_spaces_before(chars, comma_idx)
        && cfg.max_spaces_before >= 0
    {
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        if spaces_i64 > cfg.max_spaces_before {
            let (line, column) = line_and_column(line_starts, CharPos::new(comma_idx));
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
        let (line, column) = line_and_column(line_starts, CharPos::new(next_char));
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

#[doc(hidden)]
#[must_use]
pub fn coverage_overlapping_replacements() -> String {
    apply_replacements(
        "abcd",
        vec![
            (BytePos::new(1), BytePos::new(3), "X".to_string()),
            (BytePos::new(2), BytePos::new(4), "Y".to_string()),
        ],
    )
}

fn collect_comma_fixes(
    cfg: &Config,
    chars: &[(usize, char)],
    comma_idx: usize,
    replacements: &mut Vec<(BytePos, BytePos, String)>,
) {
    if let BeforeResult::SameLine { spaces, start_idx } =
        compute_spaces_before(chars, comma_idx)
        && cfg.max_spaces_before >= 0
    {
        let target = usize::try_from(cfg.max_spaces_before).unwrap_or(usize::MAX);
        if spaces > target {
            replacements.push((
                BytePos::new(chars[start_idx].0),
                BytePos::new(chars[comma_idx].0),
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
            BytePos::new(chars[comma_idx].0 + chars[comma_idx].1.len_utf8()),
            BytePos::new(chars[next_char].0),
            " ".repeat(target),
        ));
    }
}

fn target_spaces_after(cfg: &Config, current: usize) -> Option<usize> {
    let target = target_spacing(current, cfg.min_spaces_after, cfg.max_spaces_after);
    (target != current).then_some(target)
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
