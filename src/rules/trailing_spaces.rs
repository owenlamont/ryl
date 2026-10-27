//! `trailing-spaces`: report and strip trailing whitespace from lines.
//!
//! `--fix` strips space-only block-scalar lines only when their spaces are not content.
//! A double-quoted backslash + trailing whitespace + newline differs
//! from `\<newline>` alone (a line-continuation escape that drops the folded space).
//! Multi-line single-quoted and plain scalars fold trailing whitespace away, so they
//! stay fixable. The protected line set comes from `ryl_yaml_parser`, so the fix bails
//! (returns `None`) on an unparsable buffer.
use crate::rules::support::span_utils::BytePos;
use crate::text_version::edits::Edit;
use ryl_yaml_parser::ScalarStyle;
use std::collections::HashSet;

use crate::rules::support::line_syntax::{
    buffer_newline, protected_scalar_lines_with_analysis,
};
use crate::text_version::{Analysis, TextVersion};

pub const ID: &str = "trailing-spaces";
pub const MESSAGE: &str = "trailing spaces";

pub(crate) use crate::rules::quoted_strings::protection::{Protection, protection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    check_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn check_with_analysis(input: Analysis<'_>) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (idx, line, _ending) in input.lines() {
        let trimmed = line.trim_end_matches([' ', '\t']);
        if trimmed.len() < line.len() {
            violations.push(Violation {
                line: idx + 1,
                column: trimmed.chars().count() + 1,
            });
        }
    }
    if !violations.is_empty() {
        let mut content = input.content_blank_lines().clone();
        if let Some(protected) =
            protected_scalar_lines_with_analysis(input, |style, _span| {
                matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
            })
        {
            let headers: HashSet<_> =
                input.headers().iter().map(|header| header.line).collect();
            content.extend(protected.into_iter().filter(|line| {
                input
                    .get_line(line - 1)
                    .is_some_and(|text| !text.trim().is_empty())
                    && !headers.contains(line)
            }));
        }
        violations.retain(|violation| !content.contains(&violation.line));
    }
    violations
}

#[must_use]
pub fn fix(buffer: &str) -> Option<String> {
    fix_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>) -> Option<String> {
    crate::text_version::edits::render(input.text(), edits(input))
}

pub(crate) fn edits(input: Analysis<'_>) -> Vec<Edit> {
    edits_restyled(input, &crate::rules::quoted_strings::Restyled::default())
}

/// The edits once each multi-line double-quoted scalar `restyled` names by its start byte
/// is rewritten in a style whose line ends hold no escapes.
pub(crate) fn edits_restyled(
    input: Analysis<'_>,
    restyled: &crate::rules::quoted_strings::Restyled,
) -> Vec<Edit> {
    edits_protected(input, protection(input, restyled).as_ref())
}

pub(crate) fn edits_protected(
    input: Analysis<'_>,
    protection: Option<&Protection>,
) -> Vec<Edit> {
    let buffer = input.text();
    let Some(protection) = protection else {
        return Vec::new();
    };
    let mut edits = Vec::new();
    let mut offset = 0;
    let mut changed = false;

    for (idx, raw_line, ending) in input.lines() {
        let line_no = idx + 1;
        let stripped = if protection.line(line_no) {
            raw_line
        } else {
            let trimmed = raw_line.trim_end_matches([' ', '\t']);
            if trimmed.len() < raw_line.len() {
                changed = true;
            }
            trimmed
        };
        if stripped.len() < raw_line.len() {
            edits.push((
                BytePos::new(offset + stripped.len()),
                BytePos::new(offset + raw_line.len()),
                String::new(),
            ));
        }
        offset += raw_line.len() + ending.len();
    }

    if protection.keep_blank_eof() && changed {
        let end = BytePos::new(buffer.len());
        edits.push((end, end, buffer_newline(buffer).to_string()));
    }
    edits
}

pub(crate) fn content_blank_lines_with_analysis(input: Analysis<'_>) -> HashSet<usize> {
    crate::rules::quoted_strings::protection::content_blank_lines(input)
}

pub(crate) fn clears_line(line: &crate::rules::indentation::LineGeometry<'_>) -> bool {
    !line.has_tail() && line.scalar_content().horizontal_whitespace()
}
