//! `document-start` rule: require (or forbid) the `---` start marker.
//!
//! `--fix` adds missing markers or removes the first marker when the document can
//! stand alone without directives; empty documents and stream separators retain theirs.
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
    if !cfg.requires_marker() {
        return removal(input).into_iter().collect();
    }
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

fn removal(input: Analysis<'_>) -> Option<Edit> {
    let start = starts(input).next()?;
    let buffer = input.text();
    if !start.explicit
        || input.lines().take(start.line - 1).any(|(_, line, _)| {
            let line = line.trim_start_matches('\u{feff}');
            line.starts_with('%') || line.starts_with("...")
        })
    {
        return None;
    }
    let mut events = input.event_iter();
    events
        .find(|(event, _)| matches!(event, ryl_yaml_parser::Event::DocumentStart(..)))
        .expect("starts found a document start in the same event stream");
    let (event, span) = events.next()?;
    if matches!(event, ryl_yaml_parser::Event::Scalar(..)) && span.is_empty() {
        return None;
    }
    let after = start.offset + 3;
    let tail = &buffer[after..];
    let spaces = tail.len() - tail.trim_start_matches([' ', '\t']).len();
    let content = &tail[spaces..];
    if ["---", "..."].into_iter().any(|marker| {
        content.strip_prefix(marker).is_some_and(|rest| {
            rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n'])
        })
    }) {
        return None;
    }
    let ending = if content.starts_with("\r\n") {
        2
    } else {
        usize::from(content.starts_with(['\r', '\n']))
    };
    Some((
        BytePos::new(start.offset),
        BytePos::new(after + spaces + ending),
        String::new(),
    ))
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
