//! `document-end` rule: require (or forbid) the `...` end marker.
//!
//! `--fix` rewrites only `present: true`, inserting `...` before the `---` that follows
//! each implicitly ended document and appending one at the end of the stream. Removing
//! `...` (`present: false`) can collide with document boundaries, so it is not fixed.
use granit_parser::{Event, Parser};

use crate::config::YamlLintConfig;
use crate::rules::block_scalar_chomping;
use crate::rules::support::line_syntax::{
    buffer_newline, split_lines_preserve_endings,
};
use crate::rules::support::span_utils::marker_byte_offset;

pub const ID: &str = "document-end";
pub const MISSING_MESSAGE: &str = "missing document end \"...\"";
pub const FORBIDDEN_MESSAGE: &str = "found forbidden document end \"...\"";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    present: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            present: cfg.rule_option_bool(ID, "present", true),
        }
    }

    #[must_use]
    pub const fn new(present: bool) -> Self {
        Self { present }
    }

    #[must_use]
    pub const fn requires_marker(&self) -> bool {
        self.present
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    scan(buffer, *cfg).violations
}

fn scan(buffer: &str, cfg: Config) -> Scan {
    let mut result = Scan::default();
    let mut pending = false;
    for (event, span) in Parser::new_from_str(buffer).map_while(Result::ok) {
        if pending {
            let line = if matches!(event, Event::StreamEnd) {
                result.unended_at_stream_end = true;
                span.start.line().saturating_sub(1).max(1)
            } else {
                result.unended_before.push(span.start.line());
                span.start.line()
            };
            pending = false;
            result.violations.push(Violation {
                line,
                column: 1,
                message: MISSING_MESSAGE.to_string(),
            });
        } else if matches!(event, Event::DocumentEnd) {
            // granit spans explicit `...` but reports an implicit end as a zero-width point.
            let explicit =
                marker_byte_offset(span.start) < marker_byte_offset(span.end);
            pending = !explicit && cfg.requires_marker();
            if explicit && !cfg.requires_marker() {
                result.violations.push(Violation {
                    line: span.start.line(),
                    column: span.start.col() + 1,
                    message: FORBIDDEN_MESSAGE.to_string(),
                });
            }
        }
    }
    result
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let result = scan(buffer, *cfg);
    let newline = buffer_newline(buffer);
    let mut marker_lines = result.unended_before.iter().peekable();
    let mut output = String::with_capacity(buffer.len() + 4 * result.violations.len());
    for (idx, content, ending) in split_lines_preserve_endings(buffer) {
        if marker_lines.next_if_eq(&&(idx + 1)).is_some() {
            output.push_str("...");
            output.push_str(newline);
        }
        output.push_str(content);
        output.push_str(ending);
    }
    // A `\r`-terminated file already ends in a break; checking `\n` only would insert a
    // spurious blank line before `...`.
    let ends_in_break = buffer.ends_with(['\n', '\r']);
    if result.unended_at_stream_end
        && (ends_in_break || !block_scalar_chomping::ends_in_unstripped_scalar(buffer))
    {
        if !ends_in_break {
            output.push_str(newline);
        }
        output.push_str("...");
        output.push_str(newline);
    }
    (output != buffer).then_some(output)
}

#[derive(Default)]
struct Scan {
    violations: Vec<Violation>,
    unended_before: Vec<usize>,
    unended_at_stream_end: bool,
}
