//! `new-lines`: enforce one line-ending style across the file — Unix (LF), DOS
//! (CRLF), or the platform default. Mirrors yamllint's `new-lines`. Safe `--fix`
//! rewrites the endings to the configured style.
//!
//! A bare `\r` is a YAML 1.2 line break: as the file's first break it is never a
//! configurable style (`unix`/`dos`/`platform`), so it is reported wrong and `--fix`
//! rewrites it — a deliberate divergence from yamllint (its `type` has no `mac`).

use std::borrow::Cow;

use crate::config::YamlLintConfig;

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
    let (index, actual) = first_line_ending(buffer)?;
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
        match bytes[idx] {
            b'\r' if bytes.get(idx + 1) == Some(&b'\n') => {
                out.push_str(expected.as_ref());
                changed |= expected.as_ref() != "\r\n";
                idx += 2;
            }
            b'\r' => {
                // A bare `\r` is never the configured style, so emitting the
                // expected ending always changes the buffer.
                out.push_str(expected.as_ref());
                changed = true;
                idx += 1;
            }
            b'\n' => {
                out.push_str(expected.as_ref());
                changed |= expected.as_ref() != "\n";
                idx += 1;
            }
            _ => {
                let ch = buffer[idx..]
                    .chars()
                    .next()
                    .expect("idx should always point at a valid character boundary");
                out.push(ch);
                idx += ch.len_utf8();
            }
        }
    }

    changed.then_some(out)
}

fn first_line_ending(buffer: &str) -> Option<(usize, &'static str)> {
    let bytes = buffer.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\n' => return Some((idx, "\n")),
            b'\r' if bytes.get(idx + 1) == Some(&b'\n') => return Some((idx, "\r\n")),
            b'\r' => return Some((idx, "\r")),
            _ => {}
        }
        idx += 1;
    }
    None
}

fn display_sequence(input: &str) -> &'static str {
    if input == "\r\n" { "\\r\\n" } else { "\\n" }
}
