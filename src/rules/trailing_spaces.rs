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

use crate::rules::document_end;
use crate::rules::support::line_syntax::{
    buffer_newline, protected_scalar_lines_with_analysis,
};
use crate::text_version::{Analysis, TextVersion};

pub const ID: &str = "trailing-spaces";
pub const MESSAGE: &str = "trailing spaces";

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
    edits_restyled(input, |_| false)
}

/// The edits once each multi-line double-quoted scalar `restyled` names by its start byte
/// is rewritten in a style whose line ends hold no escapes.
pub(crate) fn edits_restyled(
    input: Analysis<'_>,
    restyled: impl Fn(usize) -> bool,
) -> Vec<Edit> {
    let buffer = input.text();
    let Some(mut protected) =
        crate::rules::support::line_syntax::protected_scalar_lines_with_analysis(
            input,
            |style, span| match style {
                ScalarStyle::Literal | ScalarStyle::Folded => true,
                ScalarStyle::DoubleQuoted => {
                    span.end.line() > span.start.line()
                        && !restyled(
                            crate::rules::support::span_utils::marker_byte_offset(
                                span.start,
                            )
                            .get(),
                        )
                }
                _ => false,
            },
        )
    else {
        return Vec::new();
    };
    let content = input.content_blank_lines();
    let mut keep_blank_eof = false;
    let headers = input.headers();
    let root_indents = document_end::root_explicit_content_indents_with_analysis(input);
    for header in headers {
        protected.remove(&header.line);
        let span = header.span;
        let scanner_indent = span.indent.unwrap_or_else(|| {
            // Blank-only spans have no content whose spaces need protection.
            if span.is_empty() || span.start.line() == header.line {
                usize::MAX
            } else {
                span.start.col()
            }
        });
        let root_indent = root_indents.get(&span.start.index()).copied();
        let indent = root_indent.unwrap_or(scanner_indent);
        for line in header.line + 1..=span.end.line() {
            if input.get_line(line - 1).is_some_and(|text| {
                text.len() <= indent
                    && text.bytes().all(|ch| ch == b' ')
                    && !content.contains(&line)
            }) {
                protected.remove(&line);
                if header.chomping == Some('+')
                    && line == input.line_count()
                    && !buffer.ends_with(['\n', '\r'])
                {
                    keep_blank_eof = true;
                }
            } else if root_indent.is_some()
                && input
                    .get_line(line - 1)
                    .is_some_and(|text| text.bytes().all(|ch| ch == b' '))
            {
                protected.insert(line);
            }
        }
    }
    let mut edits = Vec::new();
    let mut offset = 0;
    let mut changed = false;

    for (idx, raw_line, ending) in input.lines() {
        let line_no = idx + 1;
        let stripped = if protected.contains(&line_no) {
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

    if keep_blank_eof && changed {
        let end = BytePos::new(buffer.len());
        edits.push((end, end, buffer_newline(buffer).to_string()));
    }
    edits
}

pub(crate) fn content_blank_lines_with_analysis(input: Analysis<'_>) -> HashSet<usize> {
    let buffer = input.text();
    let mut content = HashSet::new();
    let headers = input.headers();
    let root_indents = document_end::root_explicit_content_indents_with_analysis(input);
    for header in headers {
        let header_line = input.line_content(header.line - 1);
        let indicator = header_line
            .chars()
            .skip(header.column)
            .take_while(|ch| !ch.is_whitespace() && *ch != '#')
            .find_map(|ch| ch.to_digit(10));
        let followed = header
            .span
            .end
            .byte_offset()
            .is_some_and(|offset| !buffer[offset..].trim().is_empty());
        let blank_only = followed
            && header.line < header.span.end.line()
            && input
                .line_contents_from(header.line)
                .take(header.span.end.line() - 1 - header.line)
                .all(|line| line.bytes().all(|ch| ch == b' '));
        let value_indent =
            if let Some(indent) = root_indents.get(&header.span.start.index()) {
                *indent
            } else if blank_only {
                header_line.bytes().take_while(|ch| *ch == b' ').count()
                    + indicator.map_or(1, |digit| digit as usize)
            } else {
                continue;
            };
        for line in header.line + 1..=header.span.end.line() {
            if input.get_line(line - 1).is_some_and(|text| {
                text.len() > value_indent && text.bytes().all(|ch| ch == b' ')
            }) {
                content.insert(line);
            }
        }
    }
    content
}
