//! `document-end` rule: require (or forbid) the `...` end marker.
//!
//! `--fix` rewrites only `present: true`, inserting `...` before the `---` that follows
//! each implicitly ended document and appending one at the end of the stream. Removing
//! `...` (`present: false`) can collide with document boundaries, so it is not fixed.
use crate::rules::quoted_strings::protection::{FinalBlank, final_blank};
use crate::rules::support::span_utils::BytePos;
use crate::text_version::edits::Edit;

mod input;
mod target;
pub(crate) use input::RetainedMarker;

pub(crate) fn retained_marker(line: &str) -> RetainedMarker {
    RetainedMarker::observe(line)
}

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::buffer_newline;
use crate::text_version::{Analysis, TextVersion};

pub const ID: &str = "document-end";
pub const MISSING_MESSAGE: &str = "missing document end \"...\"";
pub const FORBIDDEN_MESSAGE: &str = "found forbidden document end \"...\"";

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
    target::scan(input::boundaries(input), cfg)
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    fix_with_analysis(TextVersion::new(buffer).analysis(), *cfg)
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>, cfg: Config) -> Option<String> {
    crate::text_version::edits::render(input.text(), edits(input, cfg))
}

pub(crate) fn edits(input: Analysis<'_>, cfg: Config) -> Vec<Edit> {
    let buffer = input.text();
    let result = scan(input, cfg);
    let final_blank = if result.unended_at_stream_end {
        final_blank(input)
    } else {
        FinalBlank::Keep
    };
    let final_line = input.line_count();
    let newline = buffer_newline(buffer);
    let mut marker_lines = result.unended_before.iter().peekable();
    let mut edits = Vec::new();
    let mut offset = 0;
    for (idx, content, ending) in input.lines() {
        if marker_lines.next_if_eq(&&(idx + 1)).is_some() {
            let at = BytePos::new(offset);
            edits.push((at, at, format!("...{newline}")));
        }
        if final_blank == FinalBlank::Trim && idx + 1 == final_line {
            edits.push((
                BytePos::new(offset + content.trim_end_matches(' ').len()),
                BytePos::new(offset + content.len()),
                String::new(),
            ));
        }
        offset += content.len() + ending.len();
    }
    // A `\r`-terminated file already ends in a break; checking `\n` only would insert a
    // spurious blank line before `...`.
    let ends_in_break = buffer.ends_with(['\n', '\r']);
    if result.unended_at_stream_end && final_blank != FinalBlank::Refuse {
        let at = BytePos::new(buffer.len());
        let prefix = if ends_in_break { "" } else { newline };
        edits.push((at, at, format!("{prefix}...{newline}")));
    }
    edits
}

#[derive(Default)]
struct Scan {
    violations: Vec<Violation>,
    unended_before: Vec<usize>,
    unended_at_stream_end: bool,
}

pub(crate) fn finalize_boundary(
    source: Analysis<'_>,
    mut edit: Edit,
    final_newline: bool,
) -> Edit {
    edit.2 = target::boundary(edit.2, &input::Eof::new(source, edit.0), final_newline);
    edit
}
