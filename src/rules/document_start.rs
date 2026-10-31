//! `document-start` rule: require (or forbid) the `---` start marker.
//!
//! `--fix` rewrites only `present: true`, inserting `---` before each implicit document:
//! after the last `...` that precedes it, else at the buffer start or after a line-1
//! shebang or `#cloud-config`. Removing `---` (`present: false`) can collide with
//! document boundaries, so it is not fixed.
use crate::rules::support::span_utils::BytePos;
use crate::text_version::edits::Edit;
mod input;
mod target;
pub(crate) use input::starts;

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::buffer_newline;
use crate::text_version::{Analysis, TextVersion};

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
    check_with_analysis(TextVersion::new(buffer).analysis(), *cfg)
}

pub(crate) fn check_with_analysis(input: Analysis<'_>, cfg: Config) -> Vec<Violation> {
    scan(input, cfg).violations
}

fn scan(input: Analysis<'_>, cfg: Config) -> Scan {
    target::scan(input::starts(input), cfg)
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    fix_with_analysis(TextVersion::new(buffer).analysis(), *cfg)
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>, cfg: Config) -> Option<String> {
    crate::text_version::edits::render(input.text(), edits(input, cfg))
}

pub(crate) fn edits(input: Analysis<'_>, cfg: Config) -> Vec<Edit> {
    edits_keeping_magic(input, cfg, true)
}

/// [`edits`], placing a marker above a magic first line unless `keep_magic`.
pub(crate) fn edits_keeping_magic(
    input: Analysis<'_>,
    cfg: Config,
    keep_magic: bool,
) -> Vec<Edit> {
    let buffer = input.text();
    let content_lines = scan(input, cfg).implicit_starts;
    if content_lines.is_empty() {
        return Vec::new();
    }
    let lines: Vec<(&str, &str)> = input
        .lines()
        .map(|(_, content, ending)| (content, ending))
        .collect();
    let initial = input::Lines::new(input);
    let first_line = target::first_line(&initial, keep_magic);
    let mut marker_lines = content_lines
        .iter()
        .map(|&content_line| target::marker_line(&initial, first_line, content_line))
        .peekable();
    let newline = buffer_newline(buffer);
    let mut edits = Vec::new();
    let mut offset = 0;
    for (idx, (content, ending)) in lines.iter().enumerate() {
        if marker_lines.next_if_eq(&(idx + 1)).is_some() {
            let at = BytePos::new(offset + split_bom(content).0.len());
            edits.push((at, at, format!("---{newline}")));
        }
        offset += content.len() + ending.len();
    }
    edits
}

fn split_bom(line: &str) -> (&str, &str) {
    line.strip_prefix('\u{feff}')
        .map_or(("", line), |rest| ("\u{feff}", rest))
}

#[derive(Default)]
struct Scan {
    violations: Vec<Violation>,
    implicit_starts: Vec<usize>,
}

pub(crate) fn attaches_bom(
    line: &crate::rules::indentation::LineGeometry<'_>,
    relative: usize,
) -> bool {
    target::attaches_bom(line, relative)
}
