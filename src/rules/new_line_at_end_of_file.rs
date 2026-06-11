//! `new-line-at-end-of-file`: the file must end with a newline. Mirrors yamllint's
//! `new-line-at-end-of-file`. Safe `--fix` appends the missing newline.
//!
//! "Newline" is any YAML 1.2 line break (`\n`, `\r\n`, or a bare `\r`), so a file
//! ending in a bare `\r` is accepted; the line/column count is CR-aware.

use crate::rules::support::line_syntax::split_lines_preserve_endings;

pub const ID: &str = "new-line-at-end-of-file";
pub const MESSAGE: &str = "no new line character at the end of file";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str) -> Option<Violation> {
    if buffer.is_empty() || buffer.ends_with('\n') || buffer.ends_with('\r') {
        return None;
    }

    // A non-empty buffer not ending in a break yields >=1 line, so the loop runs and
    // overwrites these initializers.
    let mut line = 0usize;
    let mut last_line = "";
    for (idx, content, _ending) in split_lines_preserve_endings(buffer) {
        line = idx + 1;
        last_line = content;
    }

    Some(Violation {
        line,
        column: last_line.chars().count() + 1,
    })
}

#[must_use]
pub fn fix(buffer: &str, newline: &str) -> Option<String> {
    check(buffer).map(|_| format!("{buffer}{newline}"))
}
