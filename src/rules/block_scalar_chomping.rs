//! `block-scalar-chomping` rule: requires an explicit chomping indicator (`-` or `+`)
//! on every `|`/`>` block scalar header. An indentation indicator alone (`|2`) is
//! still flagged. No safe `--fix`: YAML has no explicit clip indicator, so a bare
//! `|`/`>` cannot be annotated without changing its chomping (see AGENTS.md "Rules
//! Without A Safe `--fix`").
//!
//! Detection enumerates block scalars from granit's scanner tokens
//! (`ScalarStyle::Literal`/`Folded`), so a `|`/`>` in a quoted scalar, comment, or
//! block content is never mistaken for a header. granit reports a non-empty token at
//! its first *content* line (not the header), with only blank lines between, so the
//! header is the nearest marker-bearing line *strictly above*. Empty/blank-only
//! scalars are the exception: granit places their token on the header at
//! end-of-stream but on the following node otherwise, so a token whose start column
//! is the marker uses its own line.
//!
//! Sources: YAML 1.2.2 §8.1.1.2; <https://www.yaml.info/learn/quote#chomp>.

use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};

use crate::rules::support::line_syntax::{
    block_scalar_header_marker_index, line_contents,
    strip_trailing_comment_preserving_quotes,
};

pub const ID: &str = "block-scalar-chomping";
pub const MESSAGE: &str = "missing explicit chomping indicator (\"-\" or \"+\")";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    let lines = line_contents(buffer);
    let mut violations = Vec::new();

    for token in Scanner::new(StrInput::new(buffer)) {
        let TokenType::Scalar(style, value) = token.1 else {
            continue;
        };
        if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
            continue;
        }

        let (header_line, header_text, marker_idx) = header_marker(
            &lines,
            token.0.start.line(),
            token.0.start.col(),
            value.chars().all(|ch| matches!(ch, '\n' | '\r')),
        );
        // `marker_idx` is the byte offset of the single-byte `|`/`>`; the column counts
        // characters not bytes, so a multibyte key shifts it correctly.
        let indicators = &header_text[marker_idx + 1..];
        if indicators.bytes().any(|b| matches!(b, b'-' | b'+')) {
            continue;
        }
        violations.push(Violation {
            line: header_line,
            column: header_text[..marker_idx].chars().count() + 1,
        });
    }

    violations
}

/// `token_line` is 1-based, `token_column` a 0-based character column, as granit
/// reports them. The marker check distinguishes the empty-at-end-of-stream case (token
/// on its own header); every other token takes the nearest marker-bearing line above.
fn header_marker<'a>(
    lines: &[&'a str],
    token_line: usize,
    token_column: usize,
    blank_only: bool,
) -> (usize, &'a str, usize) {
    let current = blank_only
        .then(|| lines.get(token_line - 1))
        .flatten()
        .and_then(|line| {
            let text = strip_trailing_comment_preserving_quotes(line);
            block_scalar_header_marker_index(text)
                .filter(|marker_idx| {
                    text[..*marker_idx].chars().count() == token_column
                })
                .map(|marker_idx| (token_line, text, marker_idx))
        });
    current
        .or_else(|| {
            (1..token_line).rev().find_map(|line_no| {
                let text = strip_trailing_comment_preserving_quotes(lines[line_no - 1]);
                block_scalar_header_marker_index(text).map(|idx| (line_no, text, idx))
            })
        })
        .expect("a block scalar has a marker-bearing header")
}
