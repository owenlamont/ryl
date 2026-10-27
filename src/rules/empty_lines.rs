//! `empty-lines` rule: limit consecutive blank lines (`max`/`max-start`/`max-end`).
//!
//! `--fix` preserves scalar content, but trims trailing blanks discarded by chomping.
//! An unparsable buffer is left untouched.
mod input;
mod target;
pub(crate) use input::Line;
pub(crate) use target::Runs;

pub(crate) fn projected(
    line: &crate::rules::indentation::LineGeometry<'_>,
    marker: bool,
) -> Line {
    Line::projected(line, marker)
}

use crate::rules::support::span_utils::BytePos;
use crate::text_version::edits::Edit;
use std::convert::TryFrom;

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::split_lines_preserve_endings;
use crate::text_version::{Analysis, TextVersion};

pub const ID: &str = "empty-lines";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    max: i64,
    max_start: i64,
    max_end: i64,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            max: cfg.rule_option_int(ID, "max", 2),
            max_start: cfg.rule_option_int(ID, "max-start", 0),
            max_end: cfg.rule_option_int(ID, "max-end", 0),
        }
    }

    #[must_use]
    pub const fn new(max: i64, max_start: i64, max_end: i64) -> Self {
        Self {
            max,
            max_start,
            max_end,
        }
    }

    pub(crate) const fn max(&self) -> i64 {
        self.max
    }

    pub(crate) const fn max_start(&self) -> i64 {
        self.max_start
    }

    pub(crate) const fn max_end(&self) -> i64 {
        self.max_end
    }
}

pub(crate) struct BlankLines<'a>(BlankSource<'a>);

enum BlankSource<'a> {
    Retained(Analysis<'a>),
    Selected(&'a [bool]),
}

impl<'a> BlankLines<'a> {
    pub(crate) const fn retained(source: Analysis<'a>) -> Self {
        Self(BlankSource::Retained(source))
    }
    pub(crate) const fn selected(lines: &'a [bool]) -> Self {
        Self(BlankSource::Selected(lines))
    }
    pub(crate) fn empty(&self, line: usize) -> bool {
        match self.0 {
            BlankSource::Retained(source) => source.line_is_empty(line),
            BlankSource::Selected(lines) => lines[line],
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
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    fix_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn fix_with_analysis(input: Analysis<'_>, cfg: &Config) -> Option<String> {
    crate::text_version::edits::render(input.text(), edits(input, cfg))
}

pub(crate) fn edits(input: Analysis<'_>, cfg: &Config) -> Vec<Edit> {
    let Some(protected) = preserved_scalar_lines_with_analysis(input) else {
        return Vec::new();
    };
    let mut edits = Vec::new();
    let mut runs = Runs::new(*cfg, &protected);
    for (idx, raw_line, ending) in input.lines() {
        append_blank_edits(
            input,
            runs.line(idx, Line::retained(idx, raw_line, ending)),
            &mut edits,
        );
    }
    append_blank_edits(input, runs.finish(false), &mut edits);
    edits
}

fn append_blank_edits(input: Analysis<'_>, removed: &[usize], edits: &mut Vec<Edit>) {
    if let (Some(&first), Some(&last)) = (removed.first(), removed.last()) {
        let start = input.byte_line_starts()[first];
        let end = input
            .byte_line_starts()
            .get(last + 1)
            .copied()
            .unwrap_or(input.text().len());
        edits.push((BytePos::new(start), BytePos::new(end), String::new()));
    }
}

fn preserved_scalar_lines(
    buffer: &str,
) -> Option<crate::rules::quoted_strings::protection::Preserved> {
    preserved_scalar_lines_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn preserved_scalar_lines_with_analysis(
    input: Analysis<'_>,
) -> Option<crate::rules::quoted_strings::protection::Preserved> {
    preserved_scalar_lines_with_empty(input, &BlankLines::retained(input))
}

pub(crate) fn preserved_scalar_lines_with_empty(
    input: Analysis<'_>,
    empty: &BlankLines<'_>,
) -> Option<crate::rules::quoted_strings::protection::Preserved> {
    crate::rules::quoted_strings::protection::preserved_lines(input, empty)
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut lines = split_lines_preserve_endings(buffer).peekable();
    let mut seen_nonblank = false;
    let mut run_start_line = 0usize;
    let mut run_len = 0usize;
    let mut run_is_start = false;

    while let Some((idx, content, _ending)) = lines.next() {
        if !content.is_empty() {
            seen_nonblank = true;
            continue;
        }

        if run_len == 0 {
            run_start_line = idx + 1;
            run_is_start = !seen_nonblank;
        }
        run_len += 1;

        // The splitter never emits empty content with no ending, so a blank line
        // always carries a break: the run ends when the next line is non-blank or absent.
        let next = lines.peek();
        if !next.is_some_and(|(_, content, _)| content.is_empty()) {
            finalize_run(
                buffer,
                cfg,
                run_start_line,
                run_len,
                run_is_start,
                next.is_none(),
                &mut violations,
            );
            run_len = 0;
            run_is_start = false;
        }
    }

    if !violations.is_empty()
        && let Some(protected) = preserved_scalar_lines(buffer)
    {
        violations.retain(|violation| !protected.line(violation.line - 1));
    }
    violations
}

fn finalize_run(
    buffer: &str,
    cfg: &Config,
    start_line: usize,
    length: usize,
    is_start: bool,
    is_end: bool,
    out: &mut Vec<Violation>,
) {
    if is_end && matches!(buffer, "\n" | "\r\n" | "\r") {
        return;
    }

    let allowed = if is_end {
        cfg.max_end()
    } else if is_start {
        cfg.max_start()
    } else {
        cfg.max()
    };

    let run_len = i64::try_from(length).unwrap_or(i64::MAX);
    if run_len <= allowed {
        return;
    }

    let last_line = start_line + length - 1;
    out.push(Violation {
        line: last_line,
        column: 1,
        message: format!("too many blank lines ({run_len} > {allowed})"),
    });
}

pub(crate) fn clears_stream(text: &str, config: &Config) -> bool {
    target::clears_stream(&input::Stream::new(text), config)
}

pub(crate) fn retains_final_blank<'a>(
    mut lines: impl DoubleEndedIterator<Item = crate::rules::indentation::LineGeometry<'a>>
    + ExactSizeIterator,
) -> bool {
    lines.len() > 1 && lines.next_back().is_some_and(|line| line.empty())
}
