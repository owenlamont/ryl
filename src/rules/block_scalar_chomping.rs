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

use granit_parser::{ScalarStyle, Scanner, Span, StrInput, TokenType};

use crate::rules::support::punctuation::{build_line_starts, line_and_column};
use crate::rules::support::span_utils::CharPos;

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
    let chars: Vec<char> = buffer.chars().collect();
    let line_starts = build_line_starts(&buffer.char_indices().collect::<Vec<_>>());
    let mut headers = Vec::new();
    let mut previous_end = 0;
    for token in Scanner::new(StrInput::new(buffer)).map_while(Result::ok) {
        let (span, token_type) = token.into_parts();
        match token_type {
            TokenType::Comment(_) => continue,
            TokenType::Scalar(ScalarStyle::Literal | ScalarStyle::Folded, _) => {
                let marker = header_marker(&chars, previous_end);
                let (line, column) =
                    line_and_column(&line_starts, CharPos::new(marker));
                headers.push(Header {
                    span,
                    line,
                    column,
                    chomping: chars[marker + 1..]
                        .iter()
                        .take_while(|ch| !ch.is_whitespace() && **ch != '#')
                        .copied()
                        .find(|ch| matches!(ch, '-' | '+')),
                });
            }
            _ => {}
        }
        previous_end = span.end.index();
    }
    headers
}

/// The char index of the first `|`/`>` at or past `from` outside a comment.
fn header_marker(chars: &[char], from: usize) -> usize {
    let mut in_comment = false;
    from + chars[from..]
        .iter()
        .position(|&ch| {
            in_comment = (in_comment || ch == '#') && !matches!(ch, '\n' | '\r');
            !in_comment && matches!(ch, '|' | '>')
        })
        .expect("a block scalar has a `|` or `>` header")
}

/// Whether `buffer` ends inside a block scalar that keeps or clips its final line break,
/// so a break appended after an unterminated last line joins its value.
pub(crate) fn ends_in_unstripped_scalar(buffer: &str) -> bool {
    let tokens: Vec<_> = Scanner::new(StrInput::new(buffer))
        .map_while(Result::ok)
        .map(granit_parser::Token::into_parts)
        .filter(|(_, kind)| !matches!(kind, TokenType::BlockEnd | TokenType::StreamEnd))
        .collect();
    // granit emits a header's comment after its scalar, so a comment counts only past it.
    let Some((span, TokenType::Scalar(..))) = tokens
        .iter()
        .rev()
        .find(|(_, kind)| !matches!(kind, TokenType::Comment(_)))
    else {
        return false;
    };
    let commented_after = tokens.iter().any(|(comment, kind)| {
        matches!(kind, TokenType::Comment(_))
            && comment.start.index() >= span.end.index()
    });
    !commented_after
        && headers(buffer).last().is_some_and(|header| {
            header.span.start.index() == span.start.index()
                && span.end.line() > header.line
                && header.chomping != Some('-')
        })
}
