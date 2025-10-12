use std::cmp;

use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;

pub const ID: &str = "document-end";
pub const MISSING_MESSAGE: &str = "missing document end \"...\"";
pub const FORBIDDEN_MESSAGE: &str = "found forbidden document end \"...\"";

#[must_use]
pub fn classify_document_end_marker_bytes(bytes: &[u8]) -> Option<&'static str> {
    let trimmed = trim_ascii(bytes);
    if trimmed == b"..." {
        Some("...")
    } else if trimmed == b"---" {
        Some("---")
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    present: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let present = cfg
            .rule_option(ID, "present")
            .and_then(saphyr::YamlOwned::as_bool)
            .unwrap_or(true);
        Self { present }
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
    DocumentStart,
    Other,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = DocumentEndReceiver::new(buffer, cfg);
    let _ = parser.load(&mut receiver, true);
    receiver.violations
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
            Marker::DocumentStart => {
                self.pending_stream_end_violation = false;
                self.violations.push(Violation {
                    line: span.start.line(),
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
        let start = span.start.index().min(self.source.len());
        let end = span.end.index().min(self.source.len());
        let slice = if start < end {
            &self.source.as_bytes()[start..end]
        } else {
            &[]
        };

        match classify_document_end_marker_bytes(slice) {
            Some("...") => Marker::ExplicitEnd,
            Some("---") => Marker::DocumentStart,
            _ => Marker::Other,
        }
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

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = bytes.len();

    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while start < end && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    &bytes[start..end]
}
