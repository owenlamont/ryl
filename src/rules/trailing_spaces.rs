//! `trailing-spaces`: report and strip trailing whitespace from lines.
//!
//! Safety scope for the `--fix` rewrite: lines inside literal (`|`) or folded
//! (`>`) block-scalar contexts are left untouched because their trailing
//! whitespace is part of the scalar's value, and stripping it would change the
//! parsed YAML. The diagnostic still fires on those lines — users must edit
//! them by hand.
use crate::rules::support::line_syntax::{
    BlockScalarTracker, leading_whitespace_width, split_lines_preserve_endings,
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
    let mut tracker = BlockScalarTracker::default();
    let mut output = String::with_capacity(buffer.len());
    let mut changed = false;

    for (_idx, raw_line, ending) in split_lines_preserve_endings(buffer) {
        let indent = leading_whitespace_width(raw_line);
        let content = &raw_line[indent..];

        let consumed = tracker.consume_line(indent, content);
        let trimmed = if consumed {
            raw_line
        } else {
            let stripped = raw_line.trim_end_matches([' ', '\t']);
            if stripped.len() < raw_line.len() {
                changed = true;
            }
            stripped
        };

        if !consumed {
            let new_indent = leading_whitespace_width(trimmed);
            let new_content = &trimmed[new_indent..];
            tracker.observe_indicator(new_indent, new_content);
        }

        output.push_str(trimmed);
        output.push_str(ending);
    }

    changed.then_some(output)
}
