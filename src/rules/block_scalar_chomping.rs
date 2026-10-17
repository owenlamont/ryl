//! `block-scalar-chomping` rule: requires an explicit chomping indicator (`-` or `+`)
//! on every `|`/`>` block scalar header. An indentation indicator alone (`|2`) is
//! still flagged. No safe `--fix`: YAML has no explicit clip indicator, so a bare
//! `|`/`>` cannot be annotated without changing its chomping (see the `property-tests` dev
//! skill).
//!
//! Detection enumerates block scalars from granit's scanner tokens
//! (`ScalarStyle::Literal`/`Folded`), so a `|`/`>` in a quoted scalar, comment, or
//! block content is never mistaken for a header. Only blanks and comments separate a
//! block scalar's `|`/`>` from the token before it, so the header is the first such
//! marker past that token outside a comment.
//!
//! Sources: YAML 1.2.2 §8.1.1.2; <https://www.yaml.info/learn/quote#chomp>.

use crate::text_version::lexical::Kind as LexicalKind;
use granit_parser::{ScalarStyle, Span};

use crate::rules::support::punctuation::line_and_column;
use crate::rules::support::span_utils::CharPos;
use crate::text_version::{Analysis, TextVersion};

pub const ID: &str = "block-scalar-chomping";
pub const MESSAGE: &str = "missing explicit chomping indicator (\"-\" or \"+\")";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    headers(buffer)
        .into_iter()
        .filter(|header| header.chomping.is_none())
        .map(|header| Violation {
            line: header.line,
            column: header.column,
        })
        .collect()
}

pub(crate) struct Header {
    pub(crate) span: Span,
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) chomping: Option<char>,
}

pub(crate) fn headers(buffer: &str) -> Vec<Header> {
    headers_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn headers_with_analysis(input: Analysis<'_>) -> Vec<Header> {
    let chars = input.chars();
    let line_starts = input.line_starts();
    let mut headers = Vec::new();
    let mut previous_end = 0;
    for (span, token_type) in input.tokens() {
        match token_type {
            LexicalKind::Comment { .. } => continue,
            LexicalKind::Scalar {
                style: ScalarStyle::Literal | ScalarStyle::Folded,
                ..
            } => {
                let marker = header_marker(chars, previous_end);
                let (line, column) = line_and_column(line_starts, CharPos::new(marker));
                headers.push(Header {
                    span: *span,
                    line,
                    column,
                    chomping: chars[marker + 1..]
                        .iter()
                        .map(|&(_, ch)| ch)
                        .take_while(|ch| !ch.is_whitespace() && *ch != '#')
                        .find(|ch| matches!(ch, '-' | '+')),
                });
            }
            _ => {}
        }
        previous_end = span.end.index();
    }
    headers
}

pub(crate) fn ambiguous_blank_body_line(buffer: &str) -> Option<usize> {
    ambiguous_blank_body_line_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn ambiguous_blank_body_line_with_analysis(
    input: Analysis<'_>,
) -> Option<usize> {
    let lines = input.line_contents();
    input.headers().iter().find_map(|header| {
        let indent = lines[header.line - 1]
            .bytes()
            .take_while(|ch| *ch == b' ')
            .count();
        let body: Vec<_> = lines
            .iter()
            .skip(header.line)
            .take_while(|line| {
                line.trim().is_empty()
                    || line.bytes().take_while(|ch| *ch == b' ').count() > indent
            })
            .collect();
        let blank_body: Vec<_> = lines
            .iter()
            .skip(header.line)
            .take_while(|line| line.trim().is_empty())
            .collect();
        let prefix: String = lines[header.line - 1]
            .chars()
            .take(header.column - 1)
            .collect();
        if prefix
            .find(':')
            .zip(prefix.rfind("- "))
            .is_some_and(|(colon, dash)| colon < dash)
            && blank_body.iter().any(|line| !line.is_empty())
        {
            return Some(header.line);
        }
        if blank_body.iter().any(|line| !line.is_empty())
            && lines
                .iter()
                .skip(header.line + blank_body.len())
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| {
                    let text = line.trim_start();
                    text.starts_with("---") || text.starts_with("...")
                })
        {
            return Some(header.line);
        }
        if header.chomping == Some('+')
            && body.iter().any(|line| line.trim().is_empty())
            && body.iter().any(|line| line.trim_start().starts_with('#'))
        {
            return Some(header.line);
        }
        if indent == 0 {
            return None;
        }
        let movable_dash = prefix.split_whitespace().any(|token| token == "-")
            || lines[..header.line - 1]
                .iter()
                .rev()
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| line.trim_end().ends_with('-'));
        let mut following = lines.iter().skip(header.line).peekable();
        if !following.peek().is_some_and(|line| line.trim().is_empty()) {
            return None;
        }
        if !following
            .clone()
            .take_while(|line| line.trim().is_empty())
            .any(|line| !line.is_empty())
        {
            return None;
        }
        following
            .find(|line| !line.trim().is_empty())
            .filter(|line| {
                let text = line.trim_start();
                !text.starts_with("---")
                    && !text.starts_with("...")
                    && (line.bytes().take_while(|ch| *ch == b' ').count() < indent
                        || (movable_dash
                            && line.bytes().take_while(|ch| *ch == b' ').count()
                                == indent))
            })
            .map(|_| header.line)
    })
}

/// The char index of the first `|`/`>` at or past `from` outside a comment.
fn header_marker(chars: &[(usize, char)], from: usize) -> usize {
    let mut in_comment = false;
    from + chars[from..]
        .iter()
        .position(|&(_, ch)| {
            in_comment = (in_comment || ch == '#') && !matches!(ch, '\n' | '\r');
            !in_comment && matches!(ch, '|' | '>')
        })
        .expect("a block scalar has a `|` or `>` header")
}
