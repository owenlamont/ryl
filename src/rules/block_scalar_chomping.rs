//! `block-scalar-chomping` rule: requires an explicit chomping indicator (`-` or `+`)
//! on every `|`/`>` block scalar header. An indentation indicator alone (`|2`) is
//! still flagged. No safe `--fix`: YAML has no explicit clip indicator, so a bare
//! `|`/`>` cannot be annotated without changing its chomping (see the `property-tests` dev
//! skill).
//!
//! Detection enumerates block scalars from the parser's scanner tokens
//! (`ScalarStyle::Literal`/`Folded`), so a `|`/`>` in a quoted scalar, comment, or
//! block content is never mistaken for a header. Only blanks and comments separate a
//! block scalar's `|`/`>` from the token before it, so the header is the first such
//! marker past that token outside a comment.
//!
//! Sources: YAML 1.2.2 §8.1.1.2; <https://www.yaml.info/learn/quote#chomp>.

use crate::text_version::lexical::Kind as LexicalKind;
use ryl_yaml_parser::{ScalarStyle, Span};

use crate::rules::support::punctuation::byte_line_and_column;
use crate::rules::support::span_utils::marker_byte_offset;
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
    check_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn check_with_analysis(input: Analysis<'_>) -> Vec<Violation> {
    input
        .headers()
        .iter()
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

pub(crate) fn headers_with_analysis(input: Analysis<'_>) -> Vec<Header> {
    let mut scanner = Headers::new(input);
    for (span, token_type) in input.tokens().iter() {
        scanner.push(span, token_type);
    }
    scanner.finish()
}

pub(crate) struct Headers<'a> {
    chars: &'a str,
    starts: &'a [usize],
    previous_end: usize,
    found: Vec<Header>,
}

impl<'a> Headers<'a> {
    pub(crate) fn new(input: Analysis<'a>) -> Self {
        Self {
            chars: input.text(),
            starts: input.byte_line_starts(),
            previous_end: 0,
            found: Vec::new(),
        }
    }

    pub(crate) fn finish(self) -> Vec<Header> {
        self.found
    }

    pub(crate) fn push(&mut self, span: Span, token_type: &LexicalKind) {
        let chars = self.chars;
        match token_type {
            LexicalKind::Comment { .. } => return,
            LexicalKind::Scalar {
                style: ScalarStyle::Literal | ScalarStyle::Folded,
                ..
            } => {
                let marker = header_marker(chars, self.previous_end);
                let (line, column) =
                    byte_line_and_column(chars.as_bytes(), self.starts, marker);
                self.found.push(Header {
                    span,
                    line,
                    column,
                    chomping: chars[marker + 1..]
                        .chars()
                        .take_while(|ch| !ch.is_whitespace() && *ch != '#')
                        .find(|ch| matches!(ch, '-' | '+')),
                });
            }
            _ => {}
        }
        self.previous_end = marker_byte_offset(span.end).get();
    }
}

pub(crate) fn ambiguous_blank_body_line(buffer: &str) -> Option<usize> {
    ambiguous_blank_body_line_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn ambiguous_blank_body_line_with_analysis(
    input: Analysis<'_>,
) -> Option<usize> {
    input.headers().iter().find_map(|header| {
        let indent = input
            .line_content(header.line - 1)
            .bytes()
            .take_while(|ch| *ch == b' ')
            .count();
        let body = input.line_contents_from(header.line).take_while(|line| {
            line.trim().is_empty()
                || line.bytes().take_while(|ch| *ch == b' ').count() > indent
        });
        let blank_body = input
            .line_contents_from(header.line)
            .take_while(|line| line.trim().is_empty());
        let header_text = input.line_content(header.line - 1);
        let end = header_text
            .char_indices()
            .nth(header.column - 1)
            .map_or(header_text.len(), |(at, _)| at);
        let prefix = &header_text[..end];
        if prefix
            .find(':')
            .zip(prefix.rfind("- "))
            .is_some_and(|(colon, dash)| colon < dash)
            && blank_body.clone().any(|line| !line.is_empty())
        {
            return Some(header.line);
        }
        if blank_body.clone().any(|line| !line.is_empty())
            && input
                .line_contents_from(header.line + blank_body.count())
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| {
                    let text = line.trim_start();
                    text.starts_with("---") || text.starts_with("...")
                })
        {
            return Some(header.line);
        }
        if header.chomping == Some('+')
            && body.clone().any(|line| line.trim().is_empty())
            && body.clone().any(|line| line.trim_start().starts_with('#'))
        {
            return Some(header.line);
        }
        if indent == 0 {
            return None;
        }
        let movable_dash = prefix.split_whitespace().any(|token| token == "-")
            || input
                .line_contents_range(0..header.line - 1)
                .rev()
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| line.trim_end().ends_with('-'));
        let mut following = input.line_contents_from(header.line).peekable();
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

fn header_marker(chars: &str, from: usize) -> usize {
    let mut in_comment = false;
    from + chars[from..]
        .char_indices()
        .find_map(|(offset, ch)| {
            in_comment = (in_comment || ch == '#') && !matches!(ch, '\n' | '\r');
            (!in_comment && matches!(ch, '|' | '>')).then_some(offset)
        })
        .expect("a block scalar has a `|` or `>` header")
}
