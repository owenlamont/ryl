use std::cmp;

use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;

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
        if !self.config.requires_marker() {
            self.pending_stream_end_violation = false;
            if self.slice(span) == "..." {
                self.violations.push(Violation {
                    line: span.start.line(),
                    column: span.start.col() + 1,
                    message: FORBIDDEN_MESSAGE.to_string(),
                });
            }
            return;
        }

        match self.slice(span) {
            "..." => {
                self.pending_stream_end_violation = false;
            }
            "---" => {
                self.pending_stream_end_violation = false;
                self.violations.push(Violation {
                    line: span.start.line(),
                    column: 1,
                    message: MISSING_MESSAGE.to_string(),
                });
            }
            _ => {
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

    fn slice(&self, span: Span) -> &'src str {
        let start = span.start.index().min(self.source.len());
        let end = span.end.index().min(self.source.len());
        &self.source[start..end]
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
