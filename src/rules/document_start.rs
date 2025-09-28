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
    let mut receiver = DocumentStartReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    receiver.violations
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
