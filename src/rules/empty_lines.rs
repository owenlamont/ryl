use std::convert::TryFrom;

use saphyr::YamlOwned;

use crate::config::YamlLintConfig;

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
        let max = cfg
            .rule_option(ID, "max")
            .and_then(YamlOwned::as_integer)
            .unwrap_or(2);
        let max_start = cfg
            .rule_option(ID, "max-start")
            .and_then(YamlOwned::as_integer)
            .unwrap_or(0);
        let max_end = cfg
            .rule_option(ID, "max-end")
            .and_then(YamlOwned::as_integer)
            .unwrap_or(0);

        Self {
            max,
            max_start,
            max_end,
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
    if is_end && buffer == "\n" {
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
