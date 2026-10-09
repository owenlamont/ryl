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
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};

pub const ID: &str = "document-end";
pub const MISSING_MESSAGE: &str = "missing document end \"...\"";
pub const FORBIDDEN_MESSAGE: &str = "found forbidden document end \"...\"";

/// The line of the next document's `---` when one opens at `offset`. granit points a
/// zero-width document end either at the marker or at the break before it, so the skip
/// is also what makes `line` the marker's rather than the point's. An explicit `...`
/// always arrives spanned, so only `---` reaches here. Content must be separated from
/// the marker (YAML 1.2.2 rule 203), so `--- foo` opens a document but `---foo` is a
/// plain scalar.
fn next_document_marker_line(
    source: &str,
    offset: BytePos,
    line: usize,
) -> Option<usize> {
    let rest = source.get(offset.get()..).unwrap_or_default();
    let marker = rest.trim_start_matches([' ', '\t', '\r', '\n']);
    let opens_document = marker.strip_prefix("---").is_some_and(|tail| {
        tail.is_empty() || tail.starts_with([' ', '\t', '\r', '\n'])
    });
    let skipped = rest.len() - marker.len();
    opens_document.then(|| line + rest[..skipped].matches('\n').count())
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    ExplicitEnd,
    /// Carries the marker's own line, which is not the event's where granit points the
    /// zero-width end at the break before the marker.
    DocumentStart {
        line: usize,
    },
    Other,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    scan(buffer, cfg).violations
}

fn scan<'src, 'cfg>(
    buffer: &'src str,
    cfg: &'cfg Config,
) -> DocumentEndReceiver<'src, 'cfg> {
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = DocumentEndReceiver::new(buffer, cfg);
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

struct DocumentEndReceiver<'src, 'cfg> {
    source: &'src str,
    config: &'cfg Config,
    violations: Vec<Violation>,
    pending_stream_end_violation: bool,
    /// 1-based lines of each `---` that follows an implicitly ended document.
    unended_before: Vec<usize>,
    unended_at_stream_end: bool,
}

impl<'src, 'cfg> DocumentEndReceiver<'src, 'cfg> {
    const fn new(source: &'src str, config: &'cfg Config) -> Self {
        Self {
            source,
            config,
            violations: Vec::new(),
            pending_stream_end_violation: false,
            unended_before: Vec::new(),
            unended_at_stream_end: false,
        }
    }

    fn handle_document_end(&mut self, span: Span) {
        let marker = self.marker(span);

        if !self.config.requires_marker() {
            self.pending_stream_end_violation = false;
            if matches!(marker, Marker::ExplicitEnd) {
                self.violations.push(Violation {
                    line: span.start.line(),
                    column: span.start.col() + 1,
                    message: FORBIDDEN_MESSAGE.to_string(),
                });
            }
            return;
        }

        match marker {
            Marker::ExplicitEnd => {
                self.pending_stream_end_violation = false;
            }
            Marker::DocumentStart { line } => {
                self.pending_stream_end_violation = false;
                self.unended_before.push(line);
                self.violations.push(Violation {
                    line,
                    column: 1,
                    message: MISSING_MESSAGE.to_string(),
                });
            }
            Marker::Other => {
                self.pending_stream_end_violation = true;
            }
        }
    }

    fn handle_stream_end(&mut self, span: Span) {
        if !self.config.requires_marker() || !self.pending_stream_end_violation {
            return;
        }

        self.unended_at_stream_end = true;
        let raw_line = span.start.line();
        let line = cmp::max(1, raw_line.saturating_sub(1));
        self.violations.push(Violation {
            line,
            column: 1,
            message: MISSING_MESSAGE.to_string(),
        });
        self.pending_stream_end_violation = false;
    }

    fn marker(&self, span: Span) -> Marker {
        let start = marker_byte_offset(span.start);
        // granit spans the explicit `...` and nothing else, reporting an implicit end
        // as a zero-width point that carries no marker text to inspect.
        if start < marker_byte_offset(span.end) {
            return Marker::ExplicitEnd;
        }
        next_document_marker_line(self.source, start, span.start.line())
            .map_or(Marker::Other, |line| Marker::DocumentStart { line })
    }
}

impl SpannedEventReceiver<'_> for DocumentEndReceiver<'_, '_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        match event {
            Event::DocumentEnd => self.handle_document_end(span),
            Event::StreamEnd => self.handle_stream_end(span),
            _ => {}
        }
    }
}
