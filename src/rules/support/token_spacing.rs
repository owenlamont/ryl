//! The whitespace after `-`, `?` and `:`, and before `:`, located for `colons` and
//! `hyphens`. Scanner tokens only classify a site: `BlockEntry` sits at the entry's
//! node rather than the dash, so each run is found by scanning the source from the
//! indicator and holds only spaces and tabs.
use crate::text_version::edits::Edit;

use crate::text_version::Analysis;
use std::ops::Range;

use crate::text_version::lexical::Kind as LexicalKind;

use crate::rules::support::punctuation::Columns;
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};

mod input;
mod target;

pub(crate) struct RetainedIndicators<'a>(
    input::Indicators<'a>,
    Option<&'a crate::format::layout::projection::Projection>,
);

impl<'a> RetainedIndicators<'a> {
    pub(crate) fn selected(
        mut self,
        projection: &'a crate::format::layout::projection::Projection,
    ) -> Self {
        self.1 = Some(projection);
        self
    }

    pub(crate) fn widened(&self) -> bool {
        self.0.spacing_prefixes().any(|(range, spaces)| {
            self.1.map_or(spaces, |projection| {
                projection.characters(range.start, range.end, spaces)
            }) >= 2
        })
    }
}

pub(crate) fn retained_indicators(
    input: Analysis<'_>,
    range: Range<usize>,
) -> RetainedIndicators<'_> {
    RetainedIndicators(input::Indicators::new(input, range), None)
}

pub(crate) type SpaceEdit = (BytePos, BytePos, usize);

pub(crate) struct SpaceSite {
    pub(crate) indicator: Indicator,
    pub(crate) edit: SpaceEdit,
}

#[derive(Clone, Copy)]
pub(crate) struct IndentationGap {
    line: usize,
    column: usize,
    removed: usize,
    rule: &'static str,
}

impl IndentationGap {
    pub(crate) const fn line(self) -> usize {
        self.line
    }
    pub(crate) const fn column(self) -> usize {
        self.column
    }
    pub(crate) const fn removed(self) -> usize {
        self.removed
    }
    pub(crate) const fn rule(self) -> &'static str {
        self.rule
    }
}

pub(crate) struct GapRequest {
    target: IndentationGap,
}

impl GapRequest {
    pub(crate) fn new(
        indicator: Indicator,
        line: usize,
        column: usize,
        width: usize,
    ) -> Option<Self> {
        (width > 1).then(|| Self {
            target: IndentationGap {
                line,
                column,
                removed: width - 1,
                rule: if indicator == Indicator::Dash {
                    crate::rules::hyphens::ID
                } else {
                    crate::rules::colons::ID
                },
            },
        })
    }

    pub(crate) const fn line(&self) -> usize {
        self.target.line
    }
    pub(crate) const fn column(&self) -> usize {
        self.target.column
    }

    pub(crate) fn resolve(self, spaces: usize) -> Option<IndentationGap> {
        (spaces == self.target.removed + 1).then_some(self.target)
    }
}

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
    contains_tab: bool,
    min: usize,
}

impl Site {
    pub(crate) fn exceeds(&self, tolerance: i64) -> bool {
        target::allowed(self, tolerance).is_some_and(|allowed| self.spaces > allowed)
    }
}

pub(crate) fn sites_with_analysis(input: Analysis<'_>) -> Vec<Site> {
    let mut sites = Sites::new(input);
    for (span, kind) in input.tokens().iter(input.text()) {
        sites.push(span, &kind);
    }
    sites.finish()
}

pub(crate) struct Sites<'a> {
    walk: Walk<'a>,
    found: Vec<Site>,
    previous_min: Option<usize>,
    pending: Option<(usize, usize)>,
    open: Vec<(usize, usize, usize)>,
    delivered: usize,
}

impl<'a> Sites<'a> {
    pub(crate) fn new(input: Analysis<'a>) -> Self {
        Self {
            walk: Walk {
                buffer: input.text(),
                columns: Columns::new(
                    input.text().as_bytes(),
                    input.byte_line_starts(),
                ),
                sparse: false,
            },
            found: Vec::new(),
            previous_min: None,
            pending: None,
            open: Vec::new(),
            delivered: 0,
        }
    }

    pub(crate) fn format(input: Analysis<'a>) -> Self {
        let mut sites = Self::new(input);
        sites.walk.sparse = true;
        sites
    }

    pub(crate) fn finish(self) -> Vec<Site> {
        crate::format::layout::stats::add(
            crate::format::layout::stats::Counter::Sites,
            self.found.len(),
        );
        self.found
    }

    pub(crate) fn edits_through(
        &mut self,
        line: usize,
        colons: bool,
        hyphens: bool,
    ) -> Vec<SpaceSite> {
        let stop = self.found.partition_point(|site| site.line <= line);
        let edits = target::through(&self.found[self.delivered..stop], colons, hyphens);
        self.delivered = stop;
        edits
    }

    pub(crate) fn push(
        &mut self,
        span: ryl_yaml_parser::Span,
        token: &LexicalKind<'_>,
    ) {
        let extent = input::Extent::observe(span, token);
        self.open.retain_mut(|(index, line, depth)| {
            match extent.kind {
                input::ExtentKind::Open => *depth += 1,
                input::ExtentKind::Close => *depth -= 1,
                _ => {}
            }
            let fix = target::extent(&extent, *depth, *line);
            if let Some(fix) = fix {
                self.found[*index].fix = fix;
            }
            *depth > 0 && fix != Some(Fix::Reindents)
        });
        if let Some((index, line)) = self.pending.take()
            && target::compact(&extent, line)
        {
            self.found[index].fix = Fix::Reindents;
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
        self.previous_min = target::minimum(&extent.adjacent);
    }
}

pub(crate) fn edits(
    input: Analysis<'_>,
    mode: Mode,
    tolerance: impl Fn(&Site) -> Option<i64>,
) -> Vec<Edit> {
    let sites = if mode == Mode::Format {
        input.format_sites()
    } else {
        input.sites()
    };
    sites
        .iter()
        .filter_map(|site| site_edit(mode, site, tolerance(site)?))
        .collect()
}

fn site_edit(mode: Mode, site: &Site, tolerance: i64) -> Option<Edit> {
    target::space_site(mode, site, tolerance)
        .map(|(from, to, spaces)| (from, to, " ".repeat(spaces)))
}

struct Walk<'a> {
    buffer: &'a str,
    columns: Columns<'a>,
    sparse: bool,
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

    fn site(
        &self,
        indicator: Indicator,
        before: bool,
        ws: Range<usize>,
        min: usize,
    ) -> Option<Site> {
        if self.sparse && ws.len() == min && !self.buffer[ws.clone()].contains('\t') {
            return None;
        }
        let (line, column) = self.columns.at(ws.end);
        Some(Site {
            indicator,
            before,
            fix: Fix::Safe,
            line,
            column: column.saturating_sub(1).max(1),
            spaces: ws.len(),
            contains_tab: self.buffer[ws.clone()].contains('\t'),
            min,
            ws,
        })
    }

    fn after(&self, indicator: Indicator, at: usize) -> Option<Site> {
        let mut end = at + 1;
        while matches!(self.byte_at(end), b' ' | b'\t') {
            end += 1;
        }
        let fix = target::after(input::Following::observe(self.byte_at(end)))?;
        let mut site = self.site(indicator, false, at + 1..end, 1)?;
        site.fix = fix;
        Some(site)
    }

    fn before_colon(&self, at: usize, min: Option<usize>) -> Option<Site> {
        let min = min?;
        let start = self.skip_ws_back(at);
        if start == 0 || matches!(self.byte_at(start - 1), b'\n' | b'\r') {
            return None;
        }
        self.site(Indicator::Colon, true, start..at, min)
    }
}

pub(crate) fn separate_keys(text: &str, edits: Vec<SpaceEdit>) -> Vec<SpaceEdit> {
    edits
        .into_iter()
        .filter_map(|edit| {
            target::separate(
                input::ExplicitKey::observe(text, edit.0.get(), edit.1.get()),
                edit,
            )
        })
        .collect()
}
