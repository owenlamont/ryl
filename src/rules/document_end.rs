//! `document-end` rule: require (or forbid) the `...` end marker.
//!
//! `--fix` rewrites only `present: true`, inserting `...` before the `---` that follows
//! each implicitly ended document and appending one at the end of the stream. Removing
//! `...` (`present: false`) can collide with document boundaries, so it is not fixed.
use std::cmp;

use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

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
    scan(buffer, cfg).violations
}

fn scan<'cfg>(buffer: &str, cfg: &'cfg Config) -> DocumentEndReceiver<'cfg> {
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = DocumentEndReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    receiver
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let receiver = scan(buffer, cfg);
    let newline = buffer_newline(buffer);
    let mut marker_lines = receiver.unended_before.iter().peekable();
    let mut output =
        String::with_capacity(buffer.len() + 4 * receiver.violations.len());
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
    if receiver.unended_at_stream_end
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

struct DocumentEndReceiver<'cfg> {
    config: &'cfg Config,
    violations: Vec<Violation>,
    /// The last document ended implicitly and no `---` or stream end has followed yet.
    pending: bool,
    /// 1-based lines of each `---` that follows an implicitly ended document.
    unended_before: Vec<usize>,
    unended_at_stream_end: bool,
}

impl<'cfg> DocumentEndReceiver<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            violations: Vec::new(),
            pending: false,
            unended_before: Vec::new(),
            unended_at_stream_end: false,
        }
    }

    fn handle_document_end(&mut self, span: Span) {
        // granit spans the explicit `...` and nothing else, reporting an implicit end
        // as a zero-width point.
        let explicit = marker_byte_offset(span.start) < marker_byte_offset(span.end);
        self.pending = !explicit && self.config.requires_marker();
        if explicit && !self.config.requires_marker() {
            self.violations.push(Violation {
                line: span.start.line(),
                column: span.start.col() + 1,
                message: FORBIDDEN_MESSAGE.to_string(),
            });
        }
    }

    fn report_missing(&mut self, line: usize) {
        self.pending = false;
        self.violations.push(Violation {
            line,
            column: 1,
            message: MISSING_MESSAGE.to_string(),
        });
    }
}

impl SpannedEventReceiver<'_> for DocumentEndReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        if !self.pending {
            if matches!(event, Event::DocumentEnd) {
                self.handle_document_end(span);
            }
            return;
        }
        // An implicit end is followed only by the next `---` or by the stream end.
        if matches!(event, Event::StreamEnd) {
            self.unended_at_stream_end = true;
            self.report_missing(cmp::max(1, span.start.line().saturating_sub(1)));
        } else {
            self.unended_before.push(span.start.line());
            self.report_missing(span.start.line());
        }
    }
}
