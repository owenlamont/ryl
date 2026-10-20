//! `empty-lines` rule: limit consecutive blank lines (`max`/`max-start`/`max-end`).
//!
//! `--fix` preserves scalar content, but trims trailing blanks discarded by chomping.
//! An unparsable buffer is left untouched.
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
    let mut blank_run = Vec::new();
    let mut seen_nonblank = false;
    let mut offset = 0;
    for (idx, raw_line, ending) in input.lines() {
        let end = offset + raw_line.len() + ending.len();
        if raw_line.is_empty() && !protected[idx] {
            blank_run.push(offset..end);
        } else {
            flush_blank_run(&mut edits, &mut blank_run, middle_max(seen_nonblank, cfg));
            if !raw_line.is_empty() {
                seen_nonblank = true;
            }
        }
        offset = end;
    }
    flush_blank_run(&mut edits, &mut blank_run, cfg.max_end());
    edits
}

fn preserved_scalar_lines(buffer: &str) -> Option<Vec<bool>> {
    preserved_scalar_lines_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn preserved_scalar_lines_with_analysis(
    input: Analysis<'_>,
) -> Option<Vec<bool>> {
    preserved_scalar_lines_with_empty(input, |line| input.line_is_empty(line))
}

pub(crate) fn preserved_scalar_lines_with_empty(
    input: Analysis<'_>,
    empty: impl Fn(usize) -> bool,
) -> Option<Vec<bool>> {
    if !input.parsed().valid {
        return None;
    }
    let mut protected = vec![false; input.line_count()];
    for &(start, last) in input.scalar_lines() {
        let end = last.min(protected.len());
        let start = start.saturating_sub(1).min(end);
        protected[start..end].fill(true);
    }
    for header in input.headers() {
        let blank_only_literal = input
            .line_content(header.line - 1)
            .chars()
            .nth(header.column - 1)
            == Some('|')
            && header.chomping.is_none()
            && input
                .line_contents_from(header.line)
                .take_while(|line| line.trim().is_empty())
                .count()
                >= 2;
        if header.chomping == Some('+') || blank_only_literal {
            continue;
        }
        let span = header.span;
        let end_line = (span.end.line() <= input.line_count())
            .then(|| input.line_content(span.end.line() - 1));
        let next_token = end_line.is_some_and(|line| {
            line.chars().take(span.end.col()).all(|ch| ch == ' ')
                && line.chars().nth(span.end.col()).is_some()
        });
        let last = span.end.line()
            - usize::from(span.end.col() == 0 || span.is_empty() || next_token);
        for line in (header.line + 1..=last)
            .rev()
            .take_while(|line| empty(*line - 1))
        {
            if let Some(protected) = protected.get_mut(line - 1) {
                *protected = false;
            }
        }
    }
    Some(protected)
}

fn middle_max(seen_nonblank: bool, cfg: &Config) -> i64 {
    if seen_nonblank {
        cfg.max()
    } else {
        cfg.max_start()
    }
}

fn flush_blank_run(
    edits: &mut Vec<Edit>,
    run: &mut Vec<std::ops::Range<usize>>,
    max: i64,
) {
    let keep = usize::try_from(max).unwrap_or(0);
    if let Some(first) = run.get(keep) {
        edits.push((
            BytePos::new(first.start),
            BytePos::new(
                run.last()
                    .expect("a blank run has a first removed line")
                    .end,
            ),
            String::new(),
        ));
    }
    run.clear();
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
        violations.retain(|violation| !protected[violation.line - 1]);
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
