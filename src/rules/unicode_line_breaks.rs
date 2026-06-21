//! `unicode-line-breaks` rule: flags raw NEL (U+0085), LINE SEPARATOR (U+2028) and
//! PARAGRAPH SEPARATOR (U+2029), suggesting the double-quoted escape `\N`/`\L`/`\P`.
//! No safe `--fix`: that escape is only valid inside a double-quoted scalar (see
//! AGENTS.md "Rules Without A Safe `--fix`").
//!
//! Sources: YAML 1.2.2 §5.4 (line breaks), §5.7 (escapes).

use crate::rules::support::line_syntax::split_lines_preserve_endings;

pub const ID: &str = "unicode-line-breaks";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// NEL/LS/PS are not YAML 1.2 breaks, so they stay inside line content; the column
/// counts characters, not bytes.
#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    split_lines_preserve_endings(buffer)
        .flat_map(|(line_idx, content, _)| {
            content.chars().enumerate().filter_map(move |(col_idx, ch)| {
                classify(ch).map(|(name, escape)| Violation {
                    line: line_idx + 1,
                    column: col_idx + 1,
                    message: format!(
                        "forbidden raw {name} U+{:04X}; escape as \"{escape}\" in a double-quoted scalar",
                        ch as u32
                    ),
                })
            })
        })
        .collect()
}

fn classify(ch: char) -> Option<(&'static str, &'static str)> {
    match ch {
        '\u{85}' => Some(("next line", "\\N")),
        '\u{2028}' => Some(("line separator", "\\L")),
        '\u{2029}' => Some(("paragraph separator", "\\P")),
        _ => None,
    }
}
