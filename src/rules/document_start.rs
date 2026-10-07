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
    let content_lines = scan(buffer, cfg).implicit_starts;
    if content_lines.is_empty() {
        return None;
    }
    let lines: Vec<(&str, &str)> = split_lines_preserve_endings(buffer)
        .map(|(_, content, ending)| (content, ending))
        .collect();
    let first_line = if is_magic_first_line(split_bom(lines[0].0).1) {
        2
    } else {
        1
    };
    let mut marker_lines = content_lines
        .iter()
        .map(|&content_line| marker_line(&lines, first_line, content_line))
        .peekable();
    let newline = buffer_newline(buffer);
    let mut output = String::with_capacity(buffer.len() + 4 * content_lines.len());
    for (idx, (content, ending)) in lines.iter().enumerate() {
        if marker_lines.next_if_eq(&(idx + 1)).is_some() {
            let (bom, rest) = split_bom(content);
            output.push_str(bom);
            output.push_str("---");
            output.push_str(newline);
            output.push_str(rest);
        } else {
            output.push_str(content);
        }
        output.push_str(ending);
    }
    Some(output)
}

/// The 1-based line that takes the `---` for the document whose content starts on
/// `content_line`: below the nearest `...` above it, or after a document-prefix BOM, since
/// a BOM inside a document is a syntax error.
fn marker_line(
    lines: &[(&str, &str)],
    first_line: usize,
    content_line: usize,
) -> usize {
    (first_line..=content_line)
        .rev()
        .find_map(|line| {
            let (bom, text) = split_bom(lines[line - 1].0);
            if text.split_whitespace().next() == Some("...") {
                Some(line + 1)
            } else {
                (!bom.is_empty()).then_some(line)
            }
        })
        .unwrap_or(first_line)
}

fn split_bom(line: &str) -> (&str, &str) {
    line.strip_prefix('\u{feff}')
        .map_or(("", line), |rest| ("\u{feff}", rest))
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
