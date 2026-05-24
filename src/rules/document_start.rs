//! `document-start` rule.
//!
//! Safety scope for `--fix`: only the `present: true` (default) case is
//! rewritten, and only for single-document buffers that contain no
//! `---`/`...` markers and do not begin with a `%YAML`/`%TAG` directive
//! line. Multi-document streams are skipped because a later doc's missing
//! `---` cannot be repaired by inserting at the start of the buffer
//! (inserting there would create an extra empty leading document). The
//! `present: false` case — removing existing `---` markers — is never
//! auto-fixed because removal can collide with multi-document boundaries
//! the rule does not track.
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
    let newline = first_newline(rest);
    let mut output = String::with_capacity(buffer.len() + 4);
    output.push_str(bom);
    output.push_str("---");
    output.push_str(newline);
    output.push_str(rest);
    Some(output)
}

fn starts_with_directive(buffer: &str) -> bool {
    for line in buffer.split_inclusive('\n') {
        let trimmed = line
            .trim_end_matches(['\r', '\n'])
            .trim_start_matches([' ', '\t']);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        return trimmed.starts_with('%');
    }
    false
}

fn contains_document_markers(buffer: &str) -> bool {
    buffer.split_inclusive('\n').any(|line| {
        let trimmed = line
            .trim_end_matches(['\r', '\n'])
            .trim_start_matches([' ', '\t']);
        trimmed == "---"
            || trimmed.starts_with("--- ")
            || trimmed == "..."
            || trimmed.starts_with("... ")
    })
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
