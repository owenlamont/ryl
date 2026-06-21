//! `document-start` rule: require (or forbid) the `---` start marker.
//!
//! `--fix` rewrites only `present: true` on a single-document buffer with no
//! `---`/`...` markers and no leading `%YAML`/`%TAG` directive: inserting `---` at the
//! buffer start cannot repair a later document's missing marker (it would create an
//! extra empty leading document), and removing `---` (`present: false`) can collide
//! with document boundaries, so neither is fixed.
use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::{buffer_newline, line_contents};

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
    let (bom, rest) = buffer
        .strip_prefix('\u{feff}')
        .map_or(("", buffer), |rest| ("\u{feff}", rest));
    if !cfg.requires_marker()
        || starts_with_directive(rest)
        || contains_document_markers(rest)
    {
        return None;
    }
    if check(buffer, cfg).is_empty() {
        return None;
    }
    let newline = buffer_newline(rest);
    let mut output = String::with_capacity(buffer.len() + 4);
    output.push_str(bom);
    output.push_str("---");
    output.push_str(newline);
    output.push_str(rest);
    Some(output)
}

fn starts_with_directive(buffer: &str) -> bool {
    for line in line_contents(buffer) {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return trimmed.starts_with('%');
    }
    false
}

fn contains_document_markers(buffer: &str) -> bool {
    line_contents(buffer).iter().any(|line| {
        let trimmed = line.trim_start_matches([' ', '\t']);
        trimmed == "---"
            || trimmed.starts_with("--- ")
            || trimmed == "..."
            || trimmed.starts_with("... ")
    })
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
