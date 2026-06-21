//! `trailing-spaces`: report and strip trailing whitespace from lines.
//!
//! `--fix` leaves lines inside a literal/folded block scalar or a multi-line
//! double-quoted scalar untouched: block scalars preserve trailing whitespace as
//! literal value, and a double-quoted backslash + trailing whitespace + newline differs
//! from `\<newline>` alone (a line-continuation escape that drops the folded space).
//! Multi-line single-quoted and plain scalars fold trailing whitespace away, so they
//! stay fixable. The protected line set comes from `granit_parser`, so the fix bails
//! (returns `None`) on an unparsable buffer.
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
    for (idx, line, _ending) in split_lines_preserve_endings(buffer) {
        let trimmed = line.trim_end_matches([' ', '\t']);
        if trimmed.len() < line.len() {
            violations.push(Violation {
                line: idx + 1,
                column: trimmed.chars().count() + 1,
            });
        }
    }
    violations
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
