//! `document-start` rule: require (or forbid) the `---` start marker.
//!
//! `--fix` rewrites only `present: true`, inserting `---` before each implicit document:
//! after the last `...` that precedes it, else at the buffer start or after a line-1
//! shebang or `#cloud-config`. Removing `---` (`present: false`) can collide with
//! document boundaries, so it is not fixed.
use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::{
    buffer_newline, is_magic_first_line, split_lines_preserve_endings,
};

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

fn scan<'cfg>(buffer: &str, cfg: &'cfg Config) -> DocumentStartReceiver<'cfg> {
    let mut parser = Parser::new_from_str(buffer);
    let mut receiver = DocumentStartReceiver::new(cfg);
    let _ = parser.load(&mut receiver, true);
    receiver
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let (bom, rest) = buffer
        .strip_prefix('\u{feff}')
        .map_or(("", buffer), |rest| ("\u{feff}", rest));
    let content_lines = scan(rest, cfg).implicit_starts;
    if content_lines.is_empty() {
        return None;
    }
    let lines: Vec<(&str, &str)> = split_lines_preserve_endings(rest)
        .map(|(_, content, ending)| (content, ending))
        .collect();
    let first_line = if is_magic_first_line(lines[0].0) {
        2
    } else {
        1
    };
    let mut marker_lines = content_lines
        .iter()
        .map(|&content_line| {
            (1..content_line)
                .rev()
                .find(|&line| is_document_end_line(lines[line - 1].0))
                .map_or(first_line, |line| line + 1)
        })
        .peekable();
    let newline = buffer_newline(rest);
    let mut output = String::with_capacity(buffer.len() + 4 * content_lines.len());
    output.push_str(bom);
    for (idx, (content, ending)) in lines.iter().enumerate() {
        if marker_lines.next_if_eq(&(idx + 1)).is_some() {
            output.push_str("---");
            output.push_str(newline);
        }
        output.push_str(content);
        output.push_str(ending);
    }
    Some(output)
}

/// Whether a line is a `...` end marker; the nearest one above an implicit document is
/// the line between documents it follows, even when granit emits no event for it.
fn is_document_end_line(line: &str) -> bool {
    line.split_whitespace().next() == Some("...")
}

struct DocumentStartReceiver<'cfg> {
    config: &'cfg Config,
    violations: Vec<Violation>,
    /// 1-based first content line of each implicit document.
    implicit_starts: Vec<usize>,
}

impl<'cfg> DocumentStartReceiver<'cfg> {
    const fn new(config: &'cfg Config) -> Self {
        Self {
            config,
            violations: Vec::new(),
            implicit_starts: Vec::new(),
        }
    }
}

impl SpannedEventReceiver<'_> for DocumentStartReceiver<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        if let Event::DocumentStart(explicit, _) = event {
            if self.config.requires_marker() {
                if !explicit {
                    self.implicit_starts.push(span.start.line());
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
