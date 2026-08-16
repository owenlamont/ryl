//! `document-end` rule: require (or forbid) the `...` end marker.
//!
//! `--fix` rewrites only `present: true` on a single-document buffer: multi-document
//! inputs need per-document end offsets the rule does not record, and removing `...`
//! (`present: false`) can collide with document boundaries, so neither is fixed.
use std::cmp;

use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::{buffer_newline, line_contents};
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
    pub const fn new_for_tests(present: bool) -> Self {
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
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = DocumentEndReceiver::new(buffer, cfg);
    let _ = parser.load(&mut receiver, true);
    receiver.violations
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    if !cfg.requires_marker()
        || has_inner_document_markers(buffer)
        || check(buffer, cfg).is_empty()
    {
        return None;
    }
    let newline = buffer_newline(buffer);
    let mut output = buffer.to_string();
    // A `\r`-terminated file already ends in a break; checking `\n` only would insert a
    // spurious blank line before `...`.
    if !output.ends_with('\n') && !output.ends_with('\r') {
        output.push_str(newline);
    }
    output.push_str("...");
    output.push_str(newline);
    Some(output)
}

fn has_inner_document_markers(buffer: &str) -> bool {
    let mut seen_real_content = false;
    let mut start_markers = 0u32;
    for line in line_contents(buffer) {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('%') {
            continue;
        }
        if trimmed == "..." || trimmed.starts_with("... ") {
            return true;
        }
        if trimmed == "---" || trimmed.starts_with("--- ") {
            start_markers += 1;
            if start_markers > 1 || seen_real_content {
                return true;
            }
        } else {
            seen_real_content = true;
        }
    }
    false
}

struct DocumentEndReceiver<'src, 'cfg> {
    source: &'src str,
    config: &'cfg Config,
    violations: Vec<Violation>,
    pending_stream_end_violation: bool,
}

impl<'src, 'cfg> DocumentEndReceiver<'src, 'cfg> {
    const fn new(source: &'src str, config: &'cfg Config) -> Self {
        Self {
            source,
            config,
            violations: Vec::new(),
            pending_stream_end_violation: false,
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
