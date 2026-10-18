//! `document-end` rule: require (or forbid) the `...` end marker.
//!
//! `--fix` rewrites only `present: true`, inserting `...` before the `---` that follows
//! each implicitly ended document and appending one at the end of the stream. Removing
//! `...` (`present: false`) can collide with document boundaries, so it is not fixed.
use std::collections::HashMap;

use granit_parser::Event;

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::buffer_newline;
use crate::text_version::{Analysis, TextVersion};

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
    scan(TextVersion::new(buffer).analysis(), *cfg).violations
}

fn scan(input: Analysis<'_>, cfg: Config) -> Scan {
    let mut result = Scan::default();
    let mut pending = false;
    for (event, span) in input.events() {
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
            let explicit = !span.is_empty();
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
    fix_with_analysis(TextVersion::new(buffer).analysis(), *cfg)
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>, cfg: Config) -> Option<String> {
    let buffer = input.text();
    let result = scan(input, cfg);
    let final_blank = if result.unended_at_stream_end {
        final_blank_scalar_action(input)
    } else {
        FinalBlank::Keep
    };
    let final_line = input.line_contents().len();
    let newline = buffer_newline(buffer);
    let mut marker_lines = result.unended_before.iter().peekable();
    let mut output = String::with_capacity(buffer.len() + 4 * result.violations.len());
    for (idx, content, ending) in input.lines() {
        if marker_lines.next_if_eq(&&(idx + 1)).is_some() {
            output.push_str("...");
            output.push_str(newline);
        }
        if final_blank == FinalBlank::Trim && idx + 1 == final_line {
            output.push_str(content.trim_end_matches(' '));
        } else {
            output.push_str(content);
        }
        output.push_str(ending);
    }
    // A `\r`-terminated file already ends in a break; checking `\n` only would insert a
    // spurious blank line before `...`.
    let ends_in_break = buffer.ends_with(['\n', '\r']);
    if result.unended_at_stream_end && final_blank != FinalBlank::Refuse {
        if !ends_in_break {
            output.push_str(newline);
        }
        output.push_str("...");
        output.push_str(newline);
    }
    (output != buffer).then_some(output)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinalBlank {
    Keep,
    Trim,
    Refuse,
}

fn final_blank_scalar_action(input: Analysis<'_>) -> FinalBlank {
    let lines = input.line_contents();
    let headers = input.headers();
    let root_indents = root_explicit_content_indents_with_analysis(input, &lines);
    let line = lines
        .last()
        .expect("a document with a missing end is nonempty");
    headers
        .iter()
        .find_map(|header| {
            let span = header.span;
            if header.line >= lines.len()
                || span
                    .end
                    .line()
                    .saturating_sub(usize::from(span.end.col() == 0))
                    != lines.len()
                || line.is_empty()
                || !line.bytes().all(|ch| ch == b' ')
            {
                return None;
            }
            let scanner_indent = span.indent.unwrap_or_else(|| {
                if span.is_empty() || span.start.line() == header.line {
                    usize::MAX
                } else {
                    span.start.col()
                }
            });
            let root_indent = root_indents.get(&span.start.index()).copied();
            let indent = root_indent.unwrap_or(scanner_indent);
            Some(if line.len() <= indent {
                FinalBlank::Trim
            } else if root_indent.is_some() && header.chomping.is_some() {
                FinalBlank::Refuse
            } else {
                FinalBlank::Keep
            })
        })
        .unwrap_or(FinalBlank::Keep)
}

pub(crate) fn root_explicit_content_indents_with_analysis(
    input: Analysis<'_>,
    lines: &[&str],
) -> HashMap<usize, usize> {
    let headers = input.headers();
    if headers.is_empty() {
        return HashMap::new();
    }
    let roots = &input.parsed().root_scalars;
    headers
        .iter()
        .filter(|header| {
            roots.get(&header.span.start.index()) == Some(&header.span.end.index())
        })
        .filter_map(|header| {
            lines[header.line - 1]
                .chars()
                .skip(header.column)
                .take_while(|ch| !ch.is_whitespace() && *ch != '#')
                .find_map(|ch| ch.to_digit(10).and_then(|n| n.checked_sub(1)))
                .map(|n| (header.span.start.index(), n as usize))
        })
        .collect()
}

#[derive(Default)]
struct Scan {
    violations: Vec<Violation>,
    unended_before: Vec<usize>,
    unended_at_stream_end: bool,
}
