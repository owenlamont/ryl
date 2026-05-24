//! `empty-lines` rule.
//!
//! Safety scope for `--fix`: blank-line runs that fall inside any multi-line
//! scalar (literal/folded block scalar, multi-line single- or double-quoted
//! scalar, or multi-line plain scalar) are left untouched because blank
//! lines inside such scalars contribute to the parsed value. The protected
//! line set is computed via `saphyr_parser`, so the rule bails (returns
//! `None`) when the buffer cannot be parsed. Runs outside those contexts
//! (and the leading/trailing run governed by `max-start`/`max-end`) are
//! trimmed to the configured maxima.
use std::collections::HashSet;
use std::convert::TryFrom;

use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::split_lines_preserve_endings;

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
    let protected = protected_scalar_lines(buffer)?;
    let mut output = String::with_capacity(buffer.len());
    let mut blank_run: Vec<(&str, &str)> = Vec::new();
    let mut seen_nonblank = false;
    let mut changed = false;

    for (idx, raw_line, ending) in split_lines_preserve_endings(buffer) {
        let line_no = idx + 1;
        let is_blank = raw_line.chars().all(char::is_whitespace);

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

fn protected_scalar_lines(buffer: &str) -> Option<HashSet<usize>> {
    let mut parser = Parser::new_from_str(buffer);
    let mut collector = ProtectedLineCollector {
        protected: HashSet::new(),
    };
    parser.load(&mut collector, true).ok()?;
    Some(collector.protected)
}

struct ProtectedLineCollector {
    protected: HashSet<usize>,
}

impl SpannedEventReceiver<'_> for ProtectedLineCollector {
    fn on_event(&mut self, event: Event<'_>, span: Span) {
        if matches!(event, Event::Scalar(..)) {
            let start = span.start.line();
            let end = span.end.line();
            for line in start..=end {
                self.protected.insert(line);
            }
        }
    }
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

    let mut iter = buffer.split_inclusive('\n').peekable();
    let mut seen_nonblank = false;
    let mut blank_run_len = 0usize;
    let mut blank_run_start = 0usize;
    let mut blank_run_is_start = false;
    let mut offset = 0usize;
    let total_len = buffer.len();
    let mut line_no = 1usize;

    while let Some(segment) = iter.next() {
        let seg_len = segment.len();
        let next_offset = offset + seg_len;
        let is_blank_line = matches!(segment, "\n" | "\r\n");

        if is_blank_line {
            if blank_run_len == 0 {
                blank_run_start = line_no;
                blank_run_is_start = !seen_nonblank;
            }
            blank_run_len += 1;

            let next_is_blank = iter
                .peek()
                .copied()
                .is_some_and(|next_segment| matches!(next_segment, "\n" | "\r\n"));

            if !next_is_blank {
                let is_end = next_offset == total_len;
                finalize_run(
                    buffer,
                    cfg,
                    blank_run_start,
                    blank_run_len,
                    blank_run_is_start,
                    is_end,
                    &mut violations,
                );
                blank_run_len = 0;
                blank_run_is_start = false;
            }
        } else {
            seen_nonblank = true;
        }

        offset = next_offset;

        if segment.ends_with('\n') {
            if !is_blank_line {
                seen_nonblank = true;
            }
            line_no += 1;
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
    if is_end && matches!(buffer, "\n" | "\r\n") {
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
