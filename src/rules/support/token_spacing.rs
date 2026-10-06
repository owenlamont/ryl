//! The whitespace after `-`, `?` and `:`, and before `:`, located for `colons` and
//! `hyphens`. Scanner tokens only classify a site: `BlockEntry` sits at the entry's
//! node rather than the dash, so each run is found by scanning the source from the
//! indicator and holds only spaces and tabs.

use std::ops::Range;

use granit_parser::{Scanner, Span, StrInput, TokenType};

use crate::rules::support::punctuation::{build_line_starts, line_and_column};
use crate::rules::support::span_utils::{BytePos, CharPos, apply_replacements};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Indicator {
    Dash,
    Question,
    Colon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fix {
    Safe,
    /// The indicator opens a compact block collection that continues below, whose
    /// indentation is the column the run ends at.
    Reindents,
    /// The run ends at a `,` or flow closer, whose spacing `commas`, `braces` and
    /// `brackets` own.
    Elsewhere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Lint,
    Format,
}

#[derive(Debug)]
pub(crate) struct Site {
    pub(crate) indicator: Indicator,
    pub(crate) before: bool,
    pub(crate) fix: Fix,
    pub(crate) line: usize,
    pub(crate) column: usize,
    ws: Range<usize>,
    spaces: usize,
    min: usize,
}

impl Site {
    /// The most spaces `tolerance` accepts: before `:`, never fewer than the grammar needs.
    fn allowed(&self, tolerance: i64) -> Option<usize> {
        let tolerance = usize::try_from(tolerance).ok()?;
        Some(if self.before {
            tolerance.max(self.min)
        } else {
            tolerance
        })
    }

    pub(crate) fn exceeds(&self, tolerance: i64) -> bool {
        self.allowed(tolerance)
            .is_some_and(|allowed| self.spaces > allowed)
    }
}

pub(crate) fn sites(buffer: &str) -> Vec<Site> {
    let tokens: Vec<(Span, TokenType)> = Scanner::new(StrInput::new(buffer))
        .map_while(Result::ok)
        .map(granit_parser::Token::into_parts)
        .collect();
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let walk = Walk {
        buffer,
        line_starts: build_line_starts(&chars),
        chars,
        tokens: &tokens,
    };
    let mut sites = Vec::new();
    for (idx, (span, token)) in tokens.iter().enumerate() {
        let at = span.start.index();
        match token {
            TokenType::BlockEntry => {
                let dash = walk.skip_ws_back(at).checked_sub(1);
                if let Some(dash) = dash.filter(|&dash| walk.char_at(dash) == '-') {
                    sites.extend(walk.after(Indicator::Dash, dash, idx));
                }
            }
            TokenType::Key if span.end.index() > at && walk.char_at(at) == '?' => {
                sites.extend(walk.after(Indicator::Question, at, idx));
            }
            TokenType::Value if walk.char_at(at) == ':' => {
                sites.extend(walk.before_colon(at, idx));
                sites.extend(walk.after(Indicator::Colon, at, idx));
            }
            _ => {}
        }
    }
    sites
}

/// `buffer` with each safe site `tolerance` covers respaced: to `max(tolerance, the
/// grammar minimum)` where it exceeds that, or under [`Mode::Format`] wherever it differs.
pub(crate) fn fix(
    buffer: &str,
    mode: Mode,
    tolerance: impl Fn(&Site) -> Option<i64>,
) -> Option<String> {
    let replacements: Vec<_> = sites(buffer)
        .into_iter()
        .filter(|site| site.fix == Fix::Safe)
        .filter_map(|site| {
            let target = usize::try_from(tolerance(&site)?).ok()?.max(site.min);
            let spaces = " ".repeat(target);
            let edit = match mode {
                Mode::Lint => site.spaces > target,
                Mode::Format => buffer[site.ws.clone()] != spaces,
            };
            edit.then(|| {
                (
                    BytePos::new(site.ws.start),
                    BytePos::new(site.ws.end),
                    spaces,
                )
            })
        })
        .collect();
    (!replacements.is_empty()).then(|| apply_replacements(buffer, replacements))
}

struct Walk<'a> {
    buffer: &'a str,
    chars: Vec<(usize, char)>,
    line_starts: Vec<CharPos>,
    tokens: &'a [(Span, TokenType<'a>)],
}

impl Walk<'_> {
    fn char_at(&self, idx: usize) -> char {
        self.chars.get(idx).map_or('\n', |&(_, ch)| ch)
    }

    fn byte_at(&self, idx: usize) -> usize {
        self.chars
            .get(idx)
            .map_or(self.buffer.len(), |&(byte, _)| byte)
    }

    fn skip_ws_back(&self, mut idx: usize) -> usize {
        while idx > 0 && matches!(self.char_at(idx - 1), ' ' | '\t') {
            idx -= 1;
        }
        idx
    }

    fn site(&self, indicator: Indicator, before: bool, ws: Range<usize>) -> Site {
        let (line, column) = line_and_column(&self.line_starts, CharPos::new(ws.end));
        Site {
            indicator,
            before,
            fix: Fix::Safe,
            line,
            column: column.saturating_sub(1).max(1),
            spaces: ws.len(),
            min: 1,
            ws: self.byte_at(ws.start)..self.byte_at(ws.end),
        }
    }

    /// The run after the indicator at char `at`, unless a line break, comment or the end
    /// of input follows it.
    fn after(&self, indicator: Indicator, at: usize, idx: usize) -> Option<Site> {
        let mut end = at + 1;
        while matches!(self.char_at(end), ' ' | '\t') {
            end += 1;
        }
        let next = self.char_at(end);
        if matches!(next, '\n' | '\r' | '#') {
            return None;
        }
        let mut site = self.site(indicator, false, at + 1..end);
        if matches!(next, ',' | ']' | '}') {
            site.fix = Fix::Elsewhere;
        } else if self.opens_multiline_collection(idx, site.line) {
            site.fix = Fix::Reindents;
        }
        Some(site)
    }

    /// The run before the `:` at char `at`, when a node ends on the same line before it.
    fn before_colon(&self, at: usize, idx: usize) -> Option<Site> {
        let min = match idx.checked_sub(1).map(|prev| &self.tokens[prev].1) {
            Some(
                TokenType::Scalar(..)
                | TokenType::FlowMappingEnd
                | TokenType::FlowSequenceEnd,
            ) => 0,
            // `:` is an anchor-name and tag char, so `&a: v` is a scalar `v` anchored `a:`.
            Some(TokenType::Alias(_) | TokenType::Anchor(_) | TokenType::Tag(..)) => 1,
            _ => return None,
        };
        let start = self.skip_ws_back(at);
        if start == 0 || matches!(self.char_at(start - 1), '\n' | '\r') {
            return None;
        }
        let mut site = self.site(Indicator::Colon, true, start..at);
        site.min = min;
        Some(site)
    }

    /// Whether the node after token `idx`, past its properties, is a block collection
    /// opening on `line` with content on a later one.
    fn opens_multiline_collection(&self, idx: usize, line: usize) -> bool {
        let mut rest = self.tokens[idx + 1..].iter().skip_while(|(_, token)| {
            matches!(token, TokenType::Anchor(_) | TokenType::Tag(..))
        });
        let opens = rest.next().is_some_and(|(span, token)| {
            span.start.line() == line
                && matches!(
                    token,
                    TokenType::BlockMappingStart | TokenType::BlockSequenceStart
                )
        });
        let mut depth = 1usize;
        opens
            && rest
                .take_while(|(_, token)| {
                    match token {
                        TokenType::BlockMappingStart
                        | TokenType::BlockSequenceStart => {
                            depth += 1;
                        }
                        TokenType::BlockEnd => depth -= 1,
                        _ => {}
                    }
                    depth > 0
                })
                .any(|(span, token)| {
                    !matches!(token, TokenType::Comment(_) | TokenType::BlockEnd)
                        && span.end.line() > line
                })
    }
}
