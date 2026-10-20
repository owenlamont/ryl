//! The whitespace after `-`, `?` and `:`, and before `:`, located for `colons` and
//! `hyphens`. Scanner tokens only classify a site: `BlockEntry` sits at the entry's
//! node rather than the dash, so each run is found by scanning the source from the
//! indicator and holds only spaces and tabs.
use crate::text_version::edits::Edit;

use crate::text_version::Analysis;
use std::ops::Range;

use crate::text_version::lexical::Kind as LexicalKind;
use granit_parser::ScalarStyle;

use crate::rules::support::punctuation::byte_line_and_column;
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};

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

pub(crate) fn sites_with_analysis(input: Analysis<'_>) -> Vec<Site> {
    let mut sites = Sites::new(input);
    for (span, kind) in input.tokens().iter() {
        sites.push(span, kind);
    }
    sites.finish()
}

pub(crate) struct Sites<'a> {
    walk: Walk<'a>,
    found: Vec<Site>,
    previous_min: Option<usize>,
    pending: Option<(usize, usize)>,
    open: Vec<(usize, usize, usize)>,
}

impl<'a> Sites<'a> {
    pub(crate) fn new(input: Analysis<'a>) -> Self {
        Self {
            walk: Walk {
                buffer: input.text(),
                line_starts: input.byte_line_starts(),
            },
            found: Vec::new(),
            previous_min: None,
            pending: None,
            open: Vec::new(),
        }
    }

    pub(crate) fn finish(self) -> Vec<Site> {
        self.found
    }

    pub(crate) fn push(&mut self, span: granit_parser::Span, token: &LexicalKind) {
        self.open.retain_mut(|(index, line, depth)| {
            match token {
                LexicalKind::BlockMappingStart | LexicalKind::BlockSequenceStart => {
                    *depth += 1;
                }
                LexicalKind::BlockEnd => *depth -= 1,
                _ => {}
            }
            let multiline = *depth > 0
                && match token {
                    LexicalKind::Comment { .. } | LexicalKind::BlockEnd => false,
                    LexicalKind::Scalar {
                        style: ScalarStyle::Literal | ScalarStyle::Folded,
                        ..
                    } => span.end.line() > *line,
                    _ => span.start.line() > *line,
                };
            if multiline {
                self.found[*index].fix = Fix::Reindents;
            }
            *depth > 0 && !multiline
        });
        if let Some((index, line)) = self.pending.take()
            && span.start.line() == line
            && matches!(
                token,
                LexicalKind::BlockMappingStart | LexicalKind::BlockSequenceStart
            )
        {
            self.open.push((index, line, 1));
        }
        let walk = &self.walk;
        let at = marker_byte_offset(span.start).get();
        let after = match token {
            LexicalKind::BlockEntry => {
                let dash = walk.skip_ws_back(at).checked_sub(1);
                if let Some(dash) = dash.filter(|&dash| walk.byte_at(dash) == b'-') {
                    walk.after(Indicator::Dash, dash)
                } else {
                    None
                }
            }
            LexicalKind::Key if span.end.index() > span.start.index() => {
                walk.after(Indicator::Question, at)
            }
            LexicalKind::Value => {
                self.found.extend(walk.before_colon(at, self.previous_min));
                walk.after(Indicator::Colon, at)
            }
            _ => None,
        };
        if let Some(site) = after {
            if site.fix == Fix::Safe {
                self.pending = Some((self.found.len(), site.line));
            }
            self.found.push(site);
        }
        self.previous_min = match token {
            LexicalKind::Scalar { .. }
            | LexicalKind::FlowMappingEnd
            | LexicalKind::FlowSequenceEnd => Some(0),
            LexicalKind::Alias | LexicalKind::Anchor | LexicalKind::Tag => Some(1),
            _ => None,
        };
    }
}

pub(crate) fn edits(
    input: Analysis<'_>,
    mode: Mode,
    tolerance: impl Fn(&Site) -> Option<i64>,
) -> Vec<Edit> {
    let buffer = input.text();
    input
        .sites()
        .iter()
        .filter(|site| site.fix == Fix::Safe)
        .filter_map(|site| {
            let target = usize::try_from(tolerance(site)?).ok()?.max(site.min);
            let ws = &buffer[site.ws.clone()];
            let edit = match mode {
                Mode::Lint => site.spaces > target,
                Mode::Format => ws.len() != target || ws.contains('\t'),
            };
            edit.then(|| {
                (
                    BytePos::new(site.ws.start),
                    BytePos::new(site.ws.end),
                    " ".repeat(target),
                )
            })
        })
        .collect()
}

struct Walk<'a> {
    buffer: &'a str,
    line_starts: &'a [usize],
}

impl Walk<'_> {
    fn byte_at(&self, idx: usize) -> u8 {
        self.buffer.as_bytes().get(idx).copied().unwrap_or(b'\n')
    }

    fn skip_ws_back(&self, mut idx: usize) -> usize {
        while idx > 0 && matches!(self.byte_at(idx - 1), b' ' | b'\t') {
            idx -= 1;
        }
        idx
    }

    fn site(&self, indicator: Indicator, before: bool, ws: Range<usize>) -> Site {
        let (line, column) =
            byte_line_and_column(self.buffer.as_bytes(), self.line_starts, ws.end);
        Site {
            indicator,
            before,
            fix: Fix::Safe,
            line,
            column: column.saturating_sub(1).max(1),
            spaces: ws.len(),
            min: 1,
            ws,
        }
    }

    fn after(&self, indicator: Indicator, at: usize) -> Option<Site> {
        let mut end = at + 1;
        while matches!(self.byte_at(end), b' ' | b'\t') {
            end += 1;
        }
        let next = self.byte_at(end);
        if matches!(next, b'\n' | b'\r' | b'#') {
            return None;
        }
        let mut site = self.site(indicator, false, at + 1..end);
        if matches!(next, b',' | b']' | b'}') {
            site.fix = Fix::Elsewhere;
        }
        Some(site)
    }

    fn before_colon(&self, at: usize, min: Option<usize>) -> Option<Site> {
        let min = min?;
        let start = self.skip_ws_back(at);
        if start == 0 || matches!(self.byte_at(start - 1), b'\n' | b'\r') {
            return None;
        }
        let mut site = self.site(Indicator::Colon, true, start..at);
        site.min = min;
        Some(site)
    }
}
