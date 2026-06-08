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
//! Empty / blank-only block scalars (the resolved value is only line breaks, e.g.
//! a trailing `key: |`, or a header above blank lines) are not checked: granit
//! places their token on the header at end-of-stream but on the *following* line
//! otherwise, so there is no content line to anchor a reliable header lookup. The
//! skip is a deliberate trade-off — for a truly empty scalar the chomping is also
//! degenerate, but a blank-only body does differ (`key: |` clips to `""` while
//! `key: |+` keeps `"\n"`), so that narrow case is an accepted false negative.
//! yamllint has no equivalent rule, so nothing is lost against it.
//!
//! Sources: YAML 1.2.2 §8.1.1.2 (block chomping indicator).

use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};

use crate::rules::support::line_syntax::{
    block_scalar_marker_index, strip_trailing_comment_preserving_quotes,
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
        // Skip non-block scalars and empty / blank-only block scalars: a value of
        // only line breaks means there is no content line for granit to anchor the
        // token to, so the header cannot be located reliably (granit puts the token
        // on the header at end-of-stream but on the *following* line otherwise).
        // See the module-level note; this also makes the blank-only body a
        // deliberate, accepted false negative.
        if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
            || value.chars().all(|ch| matches!(ch, '\n' | '\r'))
        {
            continue;
        }

        let (header_line, header_text, marker_idx) =
            header_marker(&lines, token.0.start.line());
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

/// Locate the header of the block scalar whose first content line is
/// `content_line` (1-based, as granit numbers lines): the nearest line *strictly
/// above* it that ends in a `|`/`>` marker. Only blank lines ever sit between a
/// header and its first content, so a scanner-confirmed non-empty block scalar
/// always has its marker-bearing header above it. Because [`granit_lines`] splits
/// on the same break set granit counts, `content_line` indexes the table directly
/// and stays in bounds. Returns the header's 1-based line number, comment-stripped
/// text, and the byte index of the marker within that text.
fn header_marker<'a>(
    lines: &[&'a str],
    content_line: usize,
) -> (usize, &'a str, usize) {
    (1..content_line)
        .rev()
        .find_map(|line_no| {
            let text = strip_trailing_comment_preserving_quotes(lines[line_no - 1]);
            block_scalar_marker_index(text).map(|idx| (line_no, text, idx))
        })
        .expect("a non-empty block scalar has a marker-bearing header above it")
}
