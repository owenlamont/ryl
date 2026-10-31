//! `commas`: spacing around `,` separators in flow collections: no space before,
//! exactly one after. Mirrors yamllint's `commas`. Safe `--fix` normalises the spacing.
mod input;
mod target;

use crate::text_version::edits::Edit;

use crate::config::YamlLintConfig;
use crate::rules::support::punctuation::{
    Columns, collect_scalar_ranges, skip_comment,
};
use crate::rules::support::span_utils::{BytePos, apply_replacements};
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
    let columns = Columns::new(input.text().as_bytes(), input.byte_line_starts());
    let mut violations = Vec::new();
    scan_commas(input, |bytes, i| {
        evaluate_comma(cfg, &mut violations, bytes, i, &columns);
    });
    violations
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    fix_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>, cfg: &Config) -> Option<String> {
    crate::text_version::edits::render(input.text(), edits(input, cfg))
}

pub(crate) fn edits(input: Analysis<'_>, cfg: &Config) -> Vec<Edit> {
    let mut replacements: Vec<crate::rules::support::token_spacing::SpaceEdit> =
        Vec::new();
    scan_commas(input, |bytes, i| {
        collect_comma_fixes(cfg, bytes, i, &mut replacements);
    });

    replacements
        .into_iter()
        .map(|(from, to, spaces)| (from, to, " ".repeat(spaces)))
        .collect()
}

fn scan_commas(input: Analysis<'_>, mut visit: impl FnMut(&[u8], usize)) {
    let bytes = input.text().as_bytes();
    let mut contexts = Vec::new();
    for &(idx, ch) in input.flow_sites() {
        match ch {
            b'[' => contexts.push(FlowKind::Sequence),
            b'{' => contexts.push(FlowKind::Mapping),
            b']' | b'}' => {
                contexts.pop();
            }
            b',' if !contexts.is_empty() => visit(bytes, idx),
            _ => {}
        }
    }
}

fn evaluate_comma(
    cfg: &Config,
    violations: &mut Vec<Violation>,
    bytes: &[u8],
    comma_idx: usize,
    columns: &Columns<'_>,
) {
    if let BeforeResult::SameLine { spaces, .. } =
        compute_spaces_before(bytes, comma_idx)
        && cfg.max_spaces_before >= 0
    {
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        if spaces_i64 > cfg.max_spaces_before {
            let (line, column) = columns.at(comma_idx);
            let highlight_column = column.saturating_sub(1).max(1);
            violations.push(Violation {
                line,
                column: highlight_column,
                message: TOO_MANY_BEFORE.to_string(),
            });
        }
    }

    if let AfterResult::SameLine { spaces, next_char } =
        compute_spaces_after(bytes, comma_idx)
    {
        let spaces_i64 = i64::try_from(spaces).unwrap_or(i64::MAX);
        let (line, column) = columns.at(next_char);
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

fn compute_spaces_before(bytes: &[u8], comma_idx: usize) -> BeforeResult {
    let mut spaces = 0usize;
    let mut idx = comma_idx;

    loop {
        let Some(prev) = idx.checked_sub(1) else {
            return BeforeResult::SameLine {
                spaces,
                start_idx: comma_idx,
            };
        };

        let ch = bytes[prev];
        if matches!(ch, b' ' | b'\t') {
            spaces += 1;
            idx = prev;
            continue;
        }
        if matches!(ch, b'\n' | b'\r') {
            return BeforeResult::Ignored;
        }
        return BeforeResult::SameLine {
            spaces,
            start_idx: idx,
        };
    }
}

fn compute_spaces_after(bytes: &[u8], comma_idx: usize) -> AfterResult {
    let mut spaces = 0usize;
    let mut idx = comma_idx + 1;
    while idx < bytes.len() {
        match bytes[idx] {
            b' ' | b'\t' => {
                spaces += 1;
                idx += 1;
            }
            b'\n' | b'\r' | b'#' => return AfterResult::Ignored,
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
    let bytes = buffer.as_bytes();
    let comma_idx = buffer
        .char_indices()
        .nth(comma_idx)
        .expect("comma index identifies a character")
        .0;
    debug_assert!(comma_idx < bytes.len());
    match compute_spaces_before(bytes, comma_idx) {
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

pub(crate) fn collect_comma_fixes(
    cfg: &Config,
    bytes: &[u8],
    comma_idx: usize,
    replacements: &mut Vec<crate::rules::support::token_spacing::SpaceEdit>,
) {
    replacements.extend(target::select(cfg, input::Initial::new(bytes, comma_idx)));
}

#[doc(hidden)]
#[must_use]
pub fn coverage_skip_comment_crlf() -> (usize, usize) {
    let chars_crlf = "#\r\n".as_bytes();
    let idx_crlf = skip_comment(chars_crlf, 0);

    let chars_cr = "#\r".as_bytes();
    let idx_cr = skip_comment(chars_cr, 0);

    (idx_crlf, idx_cr)
}

pub(crate) struct Closing<'a>(target::Closing<'a>);

impl<'a> Closing<'a> {
    pub(crate) const fn new(
        config: Option<Config>,
        preserve: bool,
        mapping: crate::rules::support::flow_collection::ClosingPolicy<'a>,
        sequence: crate::rules::support::flow_collection::ClosingPolicy<'a>,
        permission: crate::format::layout::Permission<'a>,
    ) -> Self {
        Self(target::Closing::new(
            config, preserve, mapping, sequence, permission,
        ))
    }

    pub(crate) fn select(
        &self,
        buffer: &str,
        at: usize,
        edits: &mut [crate::rules::support::token_spacing::SpaceEdit],
        closing: &mut Vec<(BytePos, BytePos)>,
    ) -> bool {
        let edit = edits.last_mut().filter(|edit| edit.0.get() == at + 1);
        let initial = crate::rules::support::flow_collection::retained_closing(
            buffer.as_bytes(),
            at,
            edit.as_ref().map(|edit| edit.1.get()),
        );
        self.0.select(&initial, edit, closing)
    }
}
