//! `trailing-spaces`: report and strip trailing whitespace from lines.
//!
//! Safety scope for the `--fix` rewrite: lines that fall inside a
//! literal/folded block scalar or a multi-line double-quoted scalar are
//! left untouched. Block scalars preserve trailing whitespace as part of
//! their literal value, and multi-line double-quoted scalars treat a
//! backslash followed by trailing whitespace and a newline differently
//! from `\<newline>` alone (the latter is a line-continuation escape
//! that drops the implicit folded space). Trailing whitespace inside
//! multi-line single-quoted and multi-line plain scalars folds away at
//! parse time, so those lines remain fixable. The protected line set is
//! computed via `granit_parser`, so the rule bails (returns `None`) when
//! the buffer cannot be parsed.
use granit_parser::ScalarStyle;

use crate::rules::support::line_syntax::{
    protected_scalar_lines, split_lines_preserve_endings,
};

pub const ID: &str = "trailing-spaces";
pub const MESSAGE: &str = "trailing spaces";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let bytes = buffer.as_bytes();
    let mut line_no = 1usize;
    let mut line_start = 0usize;
    let mut idx = 0usize;

    while idx < bytes.len() {
        if bytes[idx] == b'\n' {
            let line_end = if idx > line_start && bytes[idx - 1] == b'\r' {
                idx - 1
            } else {
                idx
            };
            process_line(buffer, line_no, line_start, line_end, &mut violations);
            idx += 1;
            line_start = idx;
            line_no += 1;
        } else {
            idx += 1;
        }
    }

    process_line(buffer, line_no, line_start, bytes.len(), &mut violations);
    violations
}

fn process_line(
    buffer: &str,
    line_no: usize,
    start: usize,
    end: usize,
    out: &mut Vec<Violation>,
) {
    if start == end {
        return;
    }

    let bytes = buffer.as_bytes();
    let mut trim_pos = end;
    while trim_pos > start {
        match bytes[trim_pos - 1] {
            b' ' | b'\t' => trim_pos -= 1,
            _ => break,
        }
    }

    if trim_pos < end {
        let column = buffer[start..trim_pos].chars().count() + 1;
        out.push(Violation {
            line: line_no,
            column,
        });
    }
}

#[must_use]
pub fn fix(buffer: &str) -> Option<String> {
    let protected = protected_scalar_lines(buffer, |style, span| match style {
        ScalarStyle::Literal | ScalarStyle::Folded => true,
        ScalarStyle::DoubleQuoted => span.end.line() > span.start.line(),
        _ => false,
    })?;
    let mut output = String::with_capacity(buffer.len());
    let mut changed = false;

    for (idx, raw_line, ending) in split_lines_preserve_endings(buffer) {
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
        output.push_str(stripped);
        output.push_str(ending);
    }

    changed.then_some(output)
}
