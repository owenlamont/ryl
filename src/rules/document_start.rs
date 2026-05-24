//! `document-start` rule.
//!
//! Safety scope for `--fix`: only the `present: true` (default) case is
//! rewritten, and only when the file does not begin with a `%YAML`/`%TAG`
//! directive line (where marker placement depends on directive ordering that
//! the rule does not record). The `present: false` case — removing existing
//! `---` markers — is never auto-fixed because removal can collide with
//! multi-document boundaries the rule does not track.
use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;

pub const ID: &str = "document-start";
pub const MISSING_MESSAGE: &str = "missing document start \"---\"";
pub const FORBIDDEN_MESSAGE: &str = "found forbidden document start \"---\"";

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

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = DocumentStartReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    receiver.violations
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    if !cfg.requires_marker() || starts_with_directive(buffer) {
        return None;
    }
    if check(buffer, cfg).is_empty() {
        return None;
    }
    let newline = first_newline(buffer);
    let mut output = String::with_capacity(buffer.len() + 4);
    output.push_str("---");
    output.push_str(newline);
    output.push_str(buffer);
    Some(output)
}

fn starts_with_directive(buffer: &str) -> bool {
    buffer
        .lines()
        .next()
        .is_some_and(|line| line.trim_start().starts_with('%'))
}

fn first_newline(buffer: &str) -> &'static str {
    if buffer.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

struct DocumentStartReceiver<'cfg> {
    config: &'cfg Config,
    violations: Vec<Violation>,
}

impl<'cfg> DocumentStartReceiver<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            violations: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for DocumentStartReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        if let Event::DocumentStart(explicit) = event {
            if self.config.requires_marker() {
                if !explicit {
                    self.violations.push(Violation {
                        line: span.start.line(),
                        column: 1,
                        message: MISSING_MESSAGE.to_string(),
                    });
                }
            } else if explicit {
                self.violations.push(Violation {
                    line: span.start.line(),
                    column: span.start.col() + 1,
                    message: FORBIDDEN_MESSAGE.to_string(),
                });
            }
        }
    }
}
