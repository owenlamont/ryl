//! `empty-lines` rule.
//!
//! Safety scope for `--fix`: blank-line runs that fall inside any multi-line
//! scalar (literal/folded block scalar, multi-line single- or double-quoted
//! scalar, or multi-line plain scalar) are left untouched because blank
//! lines inside such scalars contribute to the parsed value. The protected
//! line set is computed via `granit_parser`, so the rule bails (returns
//! `None`) when the buffer cannot be parsed. Runs outside those contexts
//! (and the leading/trailing run governed by `max-start`/`max-end`) are
//! trimmed to the configured maxima.
use std::convert::TryFrom;

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::{
    protected_scalar_lines, split_lines_preserve_endings,
};

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

    const fn max(&self) -> i64 {
        self.max
    }

    const fn max_start(&self) -> i64 {
        self.max_start
    }

    const fn max_end(&self) -> i64 {
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
    let protected = protected_scalar_lines(buffer, |_style, _span| true)?;
    let mut output = String::with_capacity(buffer.len());
    let mut blank_run: Vec<(&str, &str)> = Vec::new();
    let mut seen_nonblank = false;
    let mut changed = false;

    for (idx, raw_line, ending) in split_lines_preserve_endings(buffer) {
        let line_no = idx + 1;
        let is_blank = raw_line.is_empty();

        if is_blank && !protected.contains(&line_no) {
            blank_run.push((raw_line, ending));
        } else {
            flush_blank_run(
                &mut output,
                &mut blank_run,
                middle_max(seen_nonblank, cfg),
                &mut changed,
            );
            output.push_str(raw_line);
            output.push_str(ending);
            if !is_blank {
                seen_nonblank = true;
            }
        }
    }

    flush_blank_run(&mut output, &mut blank_run, cfg.max_end(), &mut changed);

    changed.then_some(output)
}

fn middle_max(seen_nonblank: bool, cfg: &Config) -> i64 {
    if seen_nonblank {
        cfg.max()
    } else {
        cfg.max_start()
    }
}

fn flush_blank_run(
    output: &mut String,
    run: &mut Vec<(&str, &str)>,
    max: i64,
    changed: &mut bool,
) {
    if run.is_empty() {
        return;
    }
    let allowed = usize::try_from(max).unwrap_or(0);
    let keep = run.len().min(allowed);
    for (raw_line, ending) in run.iter().take(keep) {
        output.push_str(raw_line);
        output.push_str(ending);
    }
    if keep < run.len() {
        *changed = true;
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

        // A blank line whose content is empty always carries a line break, so the
        // run ends exactly when the next line is non-blank or the buffer does; an
        // empty content with no ending is never emitted for a non-empty buffer.
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
