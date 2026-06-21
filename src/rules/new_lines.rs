//! `new-lines`: enforce one line-ending style across the file: Unix (LF), DOS
//! (CRLF), or the platform default. Mirrors yamllint's `new-lines`. Safe `--fix`
//! rewrites the endings to the configured style.
//!
//! A bare `\r` is a YAML 1.2 line break: as the file's first break it is never a
//! configurable style (`unix`/`dos`/`platform`), so it is reported wrong and `--fix`
//! rewrites it, a deliberate divergence from yamllint (its `type` has no `mac`).

use std::borrow::Cow;

use crate::config::YamlLintConfig;
use crate::rules::support::line_syntax::{first_line_break, line_break_at};

pub const ID: &str = "new-lines";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Unix,
    Dos,
    Platform,
}

impl LineKind {
    fn expected(self, platform_newline: &str) -> Cow<'_, str> {
        match self {
            Self::Unix => Cow::Borrowed("\n"),
            Self::Dos => Cow::Borrowed("\r\n"),
            Self::Platform => Cow::Owned(platform_newline.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub kind: LineKind,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        let kind = match cfg.rule_option_str(ID, "type") {
            Some("dos") => LineKind::Dos,
            Some("platform") => LineKind::Platform,
            _ => LineKind::Unix,
        };
        Self { kind }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[must_use]
pub const fn platform_newline() -> &'static str {
    core::cfg_select! {
        windows => "\r\n",
        _ => "\n",
    }
}

#[must_use]
pub fn check(buffer: &str, cfg: Config, platform_newline: &str) -> Option<Violation> {
    let expected = cfg.kind.expected(platform_newline);
    let (index, actual) = first_line_break(buffer)?;
    if actual == expected.as_ref() {
        return None;
    }

    let column = buffer[..index].chars().count() + 1;
    Some(Violation {
        line: 1,
        column,
        message: format!(
            "wrong new line character: expected {}",
            display_sequence(expected.as_ref())
        ),
    })
}

#[must_use]
pub fn expected_newline(cfg: Config, platform_newline: &str) -> Cow<'_, str> {
    cfg.kind.expected(platform_newline)
}

/// Apply safe new-line fixes to `buffer` using `cfg` and `platform_newline`.
///
/// # Panics
///
/// Panics if a byte index in `buffer` does not point at a valid UTF-8 character boundary.
#[must_use]
pub fn fix(buffer: &str, cfg: Config, platform_newline: &str) -> Option<String> {
    let expected = expected_newline(cfg, platform_newline);
    let mut out = String::with_capacity(buffer.len());
    let bytes = buffer.as_bytes();
    let mut idx = 0usize;
    let mut changed = false;

    while idx < bytes.len() {
        if let Some((len, style)) = line_break_at(bytes, idx) {
            out.push_str(expected.as_ref());
            // A bare `\r` is never a configurable style, so `expected != "\r"` always
            // holds and rewriting it always counts as a change.
            changed |= expected.as_ref() != style;
            idx += len;
        } else {
            let ch = buffer[idx..]
                .chars()
                .next()
                .expect("idx should always point at a valid character boundary");
            out.push(ch);
            idx += ch.len_utf8();
        }
    }

    changed.then_some(out)
}

fn display_sequence(input: &str) -> &'static str {
    if input == "\r\n" { "\\r\\n" } else { "\\n" }
}
