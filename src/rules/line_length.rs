//! `line-length`: lines may not exceed `max` characters (counted as characters, not
//! bytes), with an optional allowance for an unbreakable long word/URL. Mirrors
//! yamllint's `line-length`. No safe `--fix`: splitting an over-long line needs folding
//! decisions that depend on the scalar's style and whether folding is legal there.

use std::convert::TryFrom;

use granit_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::split_lines_preserve_endings;

pub const ID: &str = "line-length";

#[derive(Debug, Clone)]
pub struct Config {
    max: i64,
    allow_non_breakable_words: bool,
    allow_non_breakable_inline_mappings: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let max = cfg.rule_option_int(ID, "max", 80);
        let allow_inline =
            cfg.rule_option_bool(ID, "allow-non-breakable-inline-mappings", false);
        let allow_words =
            cfg.rule_option_bool(ID, "allow-non-breakable-words", true) || allow_inline;

        Self {
            max,
            allow_non_breakable_words: allow_words,
            allow_non_breakable_inline_mappings: allow_inline,
        }
    }

    const fn max(&self) -> i64 {
        self.max
    }

    const fn allow_non_breakable_words(&self) -> bool {
        self.allow_non_breakable_words
    }

    const fn allow_non_breakable_inline_mappings(&self) -> bool {
        self.allow_non_breakable_inline_mappings
    }

    fn diagnostic_column(&self) -> usize {
        if self.max < 0 {
            0
        } else {
            let value = self.max.saturating_add(1);
            usize::try_from(value).unwrap_or(usize::MAX)
        }
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
    let mut violations = Vec::new();
    for (idx, line, _ending) in split_lines_preserve_endings(buffer) {
        process_line(line, idx + 1, cfg, &mut violations);
    }
    violations
}

fn process_line(line: &str, line_no: usize, cfg: &Config, out: &mut Vec<Violation>) {
    if line.is_empty() {
        return;
    }

    let length = line.chars().count();
    let length_i64 = i64::try_from(length).unwrap_or(i64::MAX);

    if length_i64 <= cfg.max() {
        return;
    }

    if cfg.allow_non_breakable_words() && allows_overflow(line, cfg) {
        return;
    }

    out.push(Violation {
        line: line_no,
        column: cfg.diagnostic_column(),
        message: format!("line too long ({} > {} characters)", length, cfg.max()),
    });
}

fn allows_overflow(line: &str, cfg: &Config) -> bool {
    let mut idx = 0usize;
    let bytes = line.as_bytes();

    while idx < bytes.len() && bytes[idx] == b' ' {
        idx += 1;
    }

    if idx >= bytes.len() {
        return false;
    }

    if bytes[idx] == b'#' {
        while idx < bytes.len() && bytes[idx] == b'#' {
            idx += 1;
        }
        idx = (idx + 1).min(bytes.len());
    } else if bytes[idx] == b'-' {
        idx = (idx + 1).min(bytes.len());
        idx = (idx + 1).min(bytes.len());
    }

    if idx >= bytes.len() {
        return false;
    }

    let tail_bytes = &line.as_bytes()[idx..];
    if !tail_bytes.contains(&b' ') {
        return true;
    }

    cfg.allow_non_breakable_inline_mappings() && check_inline_mapping(line)
}

fn check_inline_mapping(line: &str) -> bool {
    let mut parser = Parser::new_from_str(line);
    let mut detector = InlineMappingDetector::new(line);
    let _ = parser.load(&mut detector, true);
    detector.allowed()
}

struct InlineMappingDetector<'a> {
    line: &'a str,
    mappings: Vec<MappingState>,
    allowed: bool,
}

impl<'a> InlineMappingDetector<'a> {
    const fn new(line: &'a str) -> Self {
        Self {
            line,
            mappings: Vec::new(),
            allowed: false,
        }
    }

    const fn allowed(&self) -> bool {
        self.allowed
    }

    fn tail_from_column(&self, column: usize) -> &str {
        self.line
            .char_indices()
            .nth(column)
            .map_or("", |(idx, _)| &self.line[idx..])
    }

    fn handle_scalar_event(&mut self, span: Span) {
        let state = self
            .mappings
            .last_mut()
            .expect("scalar event handler requires active mapping");

        if state.expect_key {
            state.expect_key = false;
            return;
        }

        state.expect_key = true;
        let single_line = span.start.line() == 1 && span.end.line() == 1;
        if single_line && !self.tail_from_column(span.start.col()).contains(' ') {
            self.allowed = true;
        }
    }
}

#[derive(Debug, Clone)]
struct MappingState {
    expect_key: bool,
}

impl SpannedEventReceiver<'_> for InlineMappingDetector<'_> {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        if self.allowed {
            return;
        }

        match event {
            Event::MappingStart(_, _, _) => {
                self.mappings.push(MappingState { expect_key: true });
            }
            Event::MappingEnd => {
                self.mappings.pop();
            }
            Event::Scalar(_, _, _, _) if self.mappings.is_empty() => {}
            Event::Scalar(_, _, _, _) => self.handle_scalar_event(span),
            Event::SequenceStart(_, _, _)
            | Event::SequenceEnd
            | Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart(_)
            | Event::DocumentEnd
            | Event::Alias(_)
            | Event::Comment(_, _)
            | Event::Nothing => {}
        }
    }
}
