use saphyr::YamlOwned;
use saphyr_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;

pub const ID: &str = "octal-values";

#[derive(Debug, Clone)]
pub struct Config {
    forbid_implicit: bool,
    forbid_explicit: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let forbid_implicit = cfg
            .rule_option(ID, "forbid-implicit-octal")
            .and_then(YamlOwned::as_bool)
            .unwrap_or(true);

        let forbid_explicit = cfg
            .rule_option(ID, "forbid-explicit-octal")
            .and_then(YamlOwned::as_bool)
            .unwrap_or(true);

        Self {
            forbid_implicit,
            forbid_explicit,
        }
    }

    const fn forbid_implicit(&self) -> bool {
        self.forbid_implicit
    }

    const fn forbid_explicit(&self) -> bool {
        self.forbid_explicit
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
    let mut receiver = OctalValuesReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    receiver.diagnostics
}

struct OctalValuesReceiver<'cfg> {
    config: &'cfg Config,
    diagnostics: Vec<Violation>,
}

impl<'cfg> OctalValuesReceiver<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            diagnostics: Vec::new(),
        }
    }

    fn handle_scalar(&mut self, span: Span, value: &str) {
        let line = span.end.line();
        let column = span.end.col() + 1;

        if self.config.forbid_implicit() && is_implicit_octal(value) {
            self.diagnostics.push(Violation {
                line,
                column,
                message: format!("forbidden implicit octal value \"{value}\""),
            });
            return;
        }

        if self.config.forbid_explicit() && is_explicit_octal(value) {
            self.diagnostics.push(Violation {
                line,
                column,
                message: format!("forbidden explicit octal value \"{value}\""),
            });
        }
    }
}

impl SpannedEventReceiver<'_> for OctalValuesReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        if let Event::Scalar(value, style, _, tag) = event {
            if tag.is_some() || !matches!(style, ScalarStyle::Plain) {
                return;
            }
            self.handle_scalar(span, value.as_ref());
        }
    }
}

fn is_implicit_octal(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() <= 1 || bytes[0] != b'0' {
        return false;
    }
    if !bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    bytes[1..].iter().all(|b| (b'0'..=b'7').contains(b))
}

fn is_explicit_octal(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() <= 2 || bytes[0] != b'0' || bytes[1] != b'o' {
        return false;
    }
    bytes[2..].iter().all(|b| (b'0'..=b'7').contains(b))
}
