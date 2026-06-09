//! `block-scalar-chomping` rule &mdash; requires an explicit chomping indicator
//! on every block scalar header (issue #257).
//!
//! A block scalar header (`|` literal or `>` folded) may carry a *chomping
//! indicator* that fixes how the scalar's trailing line breaks are handled
//! (YAML 1.2.2 §8.1.1.2): `-` (strip) removes every trailing break, `+` (keep)
//! retains them all, and a bare `|`/`>` defaults to *clip* &mdash; keep exactly
//! one final break. The clip default is implicit and easy to forget, so a stray
//! or missing trailing newline silently changes the value a consumer reads. This
//! off-by-default rule makes the author state the intent with `-` or `+`.
//!
//! An *indentation* indicator alone (`|2`) is still flagged: it is not a chomping
//! indicator, so the chomping is still the implicit clip default. YAML has no
//! explicit clip indicator (only `-` and `+` exist), so a bare `|`/`>` cannot be
//! annotated without changing its chomping &mdash; there is therefore no safe
//! `--fix` (see AGENTS.md "Rules Without A Safe `--fix`").
//!
//! Detection enumerates genuine block scalars from granit's scanner tokens
//! (`ScalarStyle::Literal`/`Folded`), so a `|`/`>` inside a quoted scalar, a
//! comment, or a literal block's own content is never mistaken for a header.
//! granit reports a non-empty block scalar's token starting at its first *content*
//! line (the header column is not exposed), and only blank lines ever sit between
//! a header and its first content, so the header is recovered as the nearest line
//! *strictly above* the content that ends in a `|`/`>` marker. The source is split
//! into lines on granit's YAML 1.2 break set (`\r\n`, `\r`, `\n`) so the token's
//! line number indexes that table directly; like every granit-based rule, a bare
//! `\r` is a line break here, whereas the whitespace byte-scanning rules count
//! `\n` only (see `unicode-line-breaks`).
//!
//! Empty / blank-only block scalars need one extra case: granit places their token
//! on the header at end-of-stream but on the following node otherwise. A token
//! whose start column is the marker therefore uses its own line; every other
//! token searches strictly above its start line, just like a non-empty scalar.
//!
//! Sources: YAML 1.2.2 §8.1.1.2 (block chomping indicator);
//! <https://www.yaml.info/learn/quote#chomp> (resolved-value examples).

use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};

use crate::rules::support::line_syntax::{
    block_scalar_header_marker_index, strip_trailing_comment_preserving_quotes,
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
    let lines = granit_lines(buffer);
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
        // `marker_idx` is the byte offset of the single-byte `|`/`>` (a char
        // boundary), and the reported column counts characters, not bytes, so a
        // multibyte key shifts the column correctly (issue #232).
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

/// Split `buffer` into line contents on granit's YAML 1.2 line-break set
/// (`\r\n`, `\r`, `\n`). Indexing this table by a granit token's 1-based line
/// number therefore lands on that token's line exactly — including when a bare
/// `\r` is the break, which granit (like every other granit-based rule) counts as
/// a line break. A trailing break produces no extra empty entry.
fn granit_lines(buffer: &str) -> Vec<&str> {
    let bytes = buffer.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut idx = 0usize;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\n' => {
                lines.push(&buffer[start..idx]);
                idx += 1;
            }
            b'\r' => {
                lines.push(&buffer[start..idx]);
                idx += if bytes.get(idx + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
            }
            _ => {
                idx += 1;
                continue;
            }
        }
        start = idx;
    }
    if start < bytes.len() {
        lines.push(&buffer[start..]);
    }
    lines
}

/// Locate the header of the block scalar whose token starts at `token_line` and
/// `token_column` (1-based line and 0-based character column, as granit reports
/// them).
/// Non-empty tokens start on their first content line, while an empty token at
/// end-of-stream starts on its own marker; empty tokens before another node start
/// on that following node. Checking for the exact marker position first
/// distinguishes the end-of-stream case, then the nearest marker-bearing line
/// strictly above is the header in every other case.
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
