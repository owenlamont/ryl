use std::cell::OnceCell;
use std::ops::Range;
use std::rc::Rc;

use crate::format::layout::{Bridge, RetainedPart, VirtualLine};
use crate::format::layout::{PlanEdit, pieces::Piece};
use crate::rules::quoted_strings;
use crate::rules::support::collection_style::{Form, FragmentPart, Plan, Structure};
use crate::rules::support::line_syntax::{
    buffer_newline, split_lines_preserve_endings,
};
use crate::text_version::Analysis;

pub(crate) type FrameRows<'a> = (Frame, Vec<VirtualLine<'a>>);

pub(crate) struct View<'source, 'plan> {
    input: Analysis<'source>,
    plan: &'plan Plan,
    edits: &'source [PlanEdit],
    facts: Option<ViewFacts>,
    geometry: OnceCell<FrameRows<'source>>,
}

struct ViewFacts {
    forms: crate::rules::support::collection_style::Forms,
    keys: Vec<usize>,
    retained: RetainedRanges,
}

impl ViewFacts {
    fn new(plan: &Plan, length: usize) -> Self {
        let mut forms = Vec::new();
        let mut keys = Vec::new();
        let mut ranges = Vec::new();
        let mut at = 0;
        for recipe in plan.selections() {
            let range = recipe.range();
            ranges.push(at..range.start.get());
            for part in recipe.fragments() {
                match part {
                    FragmentPart::Source { start, end, .. } => {
                        ranges.push(start.get()..end.get());
                    }
                    FragmentPart::Structure(Structure::Start {
                        node, form, ..
                    }) => forms.push((node, form)),
                    FragmentPart::Structure(Structure::Key(node)) => {
                        keys.push(plan.span(node).end.get());
                    }
                    _ => {}
                }
            }
            at = range.end.get();
        }
        ranges.push(at..length);
        ranges.sort_by_key(|range| range.start);
        keys.sort_unstable();
        Self {
            forms: crate::rules::support::collection_style::Forms::new(forms),
            keys,
            retained: RetainedRanges(ranges.into()),
        }
    }
}

impl<'source, 'plan> View<'source, 'plan> {
    pub(crate) fn new(
        input: Analysis<'source>,
        plan: &'plan Plan,
        edits: &'source [PlanEdit],
        observed: Option<FrameRows<'source>>,
    ) -> Self {
        let geometry = OnceCell::new();
        let facts = if let Some(observed) = observed {
            let _ = geometry.set(observed);
            None
        } else {
            Some(ViewFacts::new(plan, input.text().len()))
        };
        Self {
            input,
            plan,
            edits,
            facts,
            geometry,
        }
    }

    pub(crate) fn lines(&self) -> &FrameRows<'source> {
        self.geometry.get_or_init(|| {
            Frame::with_edits(
                self.input,
                self.input.byte_line_starts(),
                self.plan,
                self.edits,
            )
        })
    }

    pub(crate) fn into_lines(self) -> Option<FrameRows<'source>> {
        self.geometry.into_inner()
    }

    pub(crate) fn retained_ranges(&self) -> RetainedRanges {
        self.facts.as_ref().map_or_else(
            || self.lines().0.retained.clone(),
            |facts| facts.retained.clone(),
        )
    }
}

impl quoted_strings::ConvertedLayout for View<'_, '_> {
    fn forms(&self) -> &crate::rules::support::collection_style::Forms {
        self.facts
            .as_ref()
            .map_or_else(|| &self.lines().0.forms, |facts| &facts.forms)
    }

    fn generated_key(&self, end: usize) -> bool {
        let keys = self
            .facts
            .as_ref()
            .map_or_else(|| &self.lines().0.keys, |facts| &facts.keys);
        keys.binary_search(&end).is_ok()
    }

    fn column(&self, start: usize) -> usize {
        self.lines()
            .0
            .source_position(self.input, start, false)
            .column
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Position {
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) byte_column: usize,
}

#[derive(Clone, Copy)]
pub(crate) enum Symbol {
    Structure(Structure),
    Empty(quoted_strings::LayoutStyle),
}

#[derive(Clone, Copy)]
pub(crate) struct Placed {
    pub(crate) symbol: Symbol,
    pub(crate) anchor: usize,
    pub(crate) first: Position,
    pub(crate) last: Position,
}

struct SourceSite {
    range: Range<usize>,
    position: Position,
    columns: SourceColumns,
}

#[derive(Clone, Copy)]
enum SourceColumns {
    Copied(usize),
    Replaced { column: usize, byte_column: usize },
}

impl SourceSite {
    fn precedes(&self, byte: usize, end: bool) -> bool {
        self.range.start < byte || (!end && self.range.start == byte)
    }
}

pub(crate) struct PositionCursor<'a> {
    frame: &'a Frame,
    starts: usize,
    ends: usize,
}

impl PositionCursor<'_> {
    pub(crate) fn position(
        &mut self,
        input: Analysis<'_>,
        byte: usize,
        column: usize,
        end: bool,
    ) -> Position {
        let next = if end {
            &mut self.ends
        } else {
            &mut self.starts
        };
        let sites = &self.frame.lines.sources;
        debug_assert!(*next == 0 || sites[*next - 1].precedes(byte, end));
        while sites
            .get(*next)
            .is_some_and(|site| site.precedes(byte, end))
        {
            *next += 1;
        }
        let site = next
            .checked_sub(1)
            .and_then(|index| sites.get(index))
            .filter(|site| byte <= site.range.end);
        self.frame.position_at(input, byte, column, end, site)
    }
}

pub(crate) enum Chunk {
    Source { range: Range<usize>, original: bool },
    Generated(Placed),
}

pub(crate) struct Frame {
    lines: LineMap,
    chunks: Vec<Chunk>,
    changed: Vec<(usize, Form)>,
    forms: crate::rules::support::collection_style::Forms,
    keys: Vec<usize>,
    eof: Position,
    retained: RetainedRanges,
}

#[derive(Clone)]
pub(crate) struct RetainedRanges(Rc<[Range<usize>]>);

impl RetainedRanges {
    pub(crate) fn keeps(
        &self,
        start: crate::rules::support::span_utils::BytePos,
        end: crate::rules::support::span_utils::BytePos,
    ) -> bool {
        let stop = self.0.partition_point(|range| range.start <= start.get());
        stop.checked_sub(1).is_some_and(|index| {
            let range = &self.0[index];
            if start == end {
                range.start < start.get() && start.get() < range.end
            } else {
                range.start <= start.get() && end.get() <= range.end
            }
        })
    }
}

#[derive(Clone)]
pub(crate) struct LineMap {
    // Sharing the vector avoids copying the source map when guards retain it.
    sources: Rc<Vec<SourceSite>>,
    origins: Rc<[usize]>,
}

impl LineMap {
    fn site(&self, byte: usize, end: bool) -> Option<&SourceSite> {
        let stop = self
            .sources
            .partition_point(|site| site.precedes(byte, end));
        stop.checked_sub(1)
            .and_then(|index| self.sources.get(index))
            .filter(|site| byte <= site.range.end)
    }

    fn fallback_line(&self, starts: &[usize], byte: usize) -> usize {
        let original = starts.partition_point(|&at| at <= byte).saturating_sub(1);
        self.origins
            .partition_point(|&origin| origin < original)
            .min(self.origins.len())
    }

    pub(crate) fn source_line(&self, starts: &[usize], byte: usize) -> usize {
        self.site(byte, false).map_or_else(
            || self.fallback_line(starts, byte),
            |site| site.position.line,
        )
    }
}

impl Frame {
    pub(crate) fn new<'a>(
        input: Analysis<'a>,
        starts: &[usize],
        plan: &Plan,
    ) -> (Self, Vec<VirtualLine<'a>>) {
        Self::with_edits(input, starts, plan, &[])
    }

    pub(crate) fn with_edits<'a>(
        input: Analysis<'a>,
        starts: &[usize],
        plan: &Plan,
        edits: &'a [PlanEdit],
    ) -> (Self, Vec<VirtualLine<'a>>) {
        let normalized;
        let starts = if input.text().contains('\u{feff}') {
            normalized = crate::rules::indentation::normalized_line_starts(input);
            normalized.as_slice()
        } else {
            starts
        };
        let mut builder = Builder {
            input,
            starts,
            edits,
            columns: Columns::new(input.text()),
            lines: Vec::new(),
            parts: Vec::new(),
            sources: Vec::new(),
            chunks: Vec::new(),
            changed: Vec::new(),
            nodes: Vec::new(),
            keys: Vec::new(),
            retained: Vec::new(),
            origin: None,
            anchor: None,
            last_line: 0,
            prefix: 0,
            column: 0,
            bytes: 0,
            bom: false,
        };
        let newline = buffer_newline(input.text());
        let mut at = 0;
        for recipe in plan.selections() {
            let range = recipe.range();
            builder.copy_original(at..range.start.get());
            for part in recipe.fragments() {
                match part {
                    FragmentPart::Source { start, end, node } => {
                        builder.copy_entry(start.get()..end.get(), node, plan);
                    }
                    FragmentPart::Text(text) => builder.text(text),
                    FragmentPart::Spaces(spaces) => builder.spaces(spaces),
                    FragmentPart::Break(column) => {
                        let original = builder.source_line(range.start.get());
                        builder.flush(newline, original);
                        builder.prefix = column;
                        builder.column = column;
                        builder.bytes = column;
                    }
                    FragmentPart::Structure(structure) => {
                        builder.structure(structure, plan);
                    }
                }
            }
            at = range.end.get();
        }
        builder.copy_original(at..input.text().len());
        let eof = builder.position();
        if builder.column != 0 || !builder.parts.is_empty() || builder.bom {
            let original = builder.source_line(input.text().len());
            builder.flush("", original);
        }
        if !builder.sources.is_sorted_by_key(|site| site.range.start) {
            builder.sources.sort_by_key(|site| site.range.start);
        }
        // Both position lookups select the last source site at a given start.
        builder.sources.dedup_by(|next, previous| {
            let duplicate = next.range.start == previous.range.start;
            if duplicate {
                std::mem::swap(next, previous);
            }
            duplicate
        });
        builder.sources.shrink_to_fit();
        builder.changed.sort_by_key(|&(at, _)| at);
        builder.nodes.sort_by_key(|&(node, _)| node);
        builder.keys.sort_unstable();
        builder.retained.sort_by_key(|range| range.start);
        let projection = Self {
            lines: LineMap {
                sources: Rc::new(builder.sources),
                origins: builder.lines.iter().map(|line| line.origin).collect(),
            },
            chunks: builder.chunks,
            changed: builder.changed,
            forms: crate::rules::support::collection_style::Forms::new(builder.nodes),
            keys: builder.keys,
            eof,
            retained: RetainedRanges(builder.retained.into()),
        };
        (projection, builder.lines)
    }

    pub(crate) fn positions(&self) -> PositionCursor<'_> {
        PositionCursor {
            frame: self,
            starts: 0,
            ends: 0,
        }
    }

    #[inline]
    fn position_at(
        &self,
        input: Analysis<'_>,
        byte: usize,
        column: usize,
        end: bool,
        site: Option<&SourceSite>,
    ) -> Position {
        if byte == input.text().len() && !end {
            return self.eof;
        }
        if let Some(site) = site {
            return match site.columns {
                SourceColumns::Copied(original) => Position {
                    line: site.position.line,
                    byte_column: site.position.byte_column
                        + byte.saturating_sub(site.range.start),
                    column: site.position.column + column.saturating_sub(original),
                },
                SourceColumns::Replaced {
                    column,
                    byte_column,
                } => {
                    if end && byte == site.range.end {
                        Position {
                            line: site.position.line,
                            column,
                            byte_column,
                        }
                    } else {
                        site.position
                    }
                }
            };
        }
        Position {
            line: self.lines.fallback_line(input.byte_line_starts(), byte),
            column,
            byte_column: column,
        }
    }

    pub(crate) fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    pub(crate) fn source_position(
        &self,
        input: Analysis<'_>,
        byte: usize,
        end: bool,
    ) -> Position {
        let site = self.lines.site(byte, end);
        let column = site.map_or(0, |site| match site.columns {
            SourceColumns::Copied(original) => {
                original + input.text()[site.range.start..byte].chars().count()
            }
            SourceColumns::Replaced { .. } => 0,
        });
        self.position_at(input, byte, column, end, site)
    }

    pub(crate) fn collection_form(&self, node: usize) -> Option<Form> {
        self.forms.at(node)
    }

    pub(crate) fn form_at(&self, byte: usize) -> Option<Form> {
        self.changed
            .binary_search_by_key(&byte, |&(at, _)| at)
            .ok()
            .map(|index| self.changed[index].1)
    }

    pub(crate) fn line_map(&self) -> LineMap {
        self.lines.clone()
    }

    pub(crate) fn retains_edit(
        &self,
        start: crate::rules::support::span_utils::BytePos,
        end: crate::rules::support::span_utils::BytePos,
    ) -> bool {
        self.retained.keeps(start, end)
    }
}

struct Columns(Vec<(usize, usize)>);

impl Columns {
    fn new(text: &str) -> Self {
        let mut extra = 0;
        Self(
            text.char_indices()
                .filter_map(|(byte, ch)| {
                    extra += ch.len_utf8() - 1;
                    (!ch.is_ascii()).then_some((byte + ch.len_utf8(), extra))
                })
                .collect(),
        )
    }

    fn extra(&self, byte: usize) -> usize {
        self.0
            .partition_point(|&(end, _)| end <= byte)
            .checked_sub(1)
            .map_or(0, |index| self.0[index].1)
    }

    fn characters(&self, range: Range<usize>) -> usize {
        range.end - range.start - (self.extra(range.end) - self.extra(range.start))
    }
}

struct Builder<'a, 'b> {
    input: Analysis<'a>,
    starts: &'b [usize],
    edits: &'a [PlanEdit],
    columns: Columns,
    lines: Vec<VirtualLine<'a>>,
    parts: Vec<RetainedPart<'a>>,
    sources: Vec<SourceSite>,
    chunks: Vec<Chunk>,
    changed: Vec<(usize, Form)>,
    nodes: Vec<(usize, Form)>,
    keys: Vec<usize>,
    retained: Vec<Range<usize>>,
    origin: Option<usize>,
    anchor: Option<usize>,
    last_line: usize,
    prefix: usize,
    column: usize,
    bytes: usize,
    bom: bool,
}

impl<'a> Builder<'a, '_> {
    fn position(&self) -> Position {
        Position {
            line: self.lines.len(),
            column: self.column,
            byte_column: self.bytes,
        }
    }

    fn source_line(&mut self, byte: usize) -> usize {
        let starts = self.input.byte_line_starts();
        if starts[self.last_line] > byte
            || starts
                .get(self.last_line + 1)
                .is_some_and(|&start| start <= byte)
        {
            self.last_line = starts
                .partition_point(|&start| start <= byte)
                .saturating_sub(1);
        }
        self.last_line
    }

    fn text(&mut self, text: &'a str) {
        self.column += text.chars().count();
        self.bytes += text.len();
        self.parts.push(RetainedPart::Text(text));
    }

    fn spaces(&mut self, count: usize) {
        self.column += count;
        self.bytes += count;
        if self.parts.is_empty() {
            self.prefix += count;
        } else {
            self.parts.push(RetainedPart::Spaces(count));
        }
    }

    fn source(&mut self, mut range: Range<usize>) {
        let original = self.source_line(range.start);
        self.origin.get_or_insert(original);
        self.anchor.get_or_insert(range.start);
        if self.input.byte_line_starts()[original] == range.start
            && self.starts[original] > range.start
        {
            self.bom = true;
            range.start = self.starts[original];
        }
        if self.parts.is_empty() {
            let text = &self.input.text()[range.clone()];
            let leading = text.len() - text.trim_start_matches(' ').len();
            self.prefix += leading;
            self.column += leading;
            self.bytes += leading;
            range.start += leading;
        }
        let position = self.position();
        self.sources.push(SourceSite {
            columns: SourceColumns::Copied(
                self.columns.characters(self.starts[original]..range.start),
            ),
            range: range.clone(),
            position,
        });
        self.column += self.columns.characters(range.clone());
        self.bytes += range.len();
        if !range.is_empty() {
            self.parts
                .push(RetainedPart::Source(&self.input.text()[range]));
        }
    }

    fn copy_original(&mut self, range: Range<usize>) {
        self.chunks.push(Chunk::Source {
            range: range.clone(),
            original: true,
        });
        self.copy(range);
    }

    fn copy_entry(&mut self, range: Range<usize>, node: Option<usize>, plan: &Plan) {
        let empty = node.and_then(|node| {
            plan.span(node)
                .is_empty()
                .then(|| plan.scalar_style(node))
                .flatten()
                .map(|style| (node, style))
        });
        if !self.input.text()[range.clone()].starts_with('#') {
            self.chunks.push(Chunk::Source {
                range: range.clone(),
                original: false,
            });
        }
        self.copy(range);
        if let Some((node, style)) = empty {
            let at = plan.span(node).start.get();
            self.token(
                Symbol::Empty(quoted_strings::retained_layout_style(style)),
                at,
                0,
            );
        }
    }

    fn copy(&mut self, range: Range<usize>) {
        self.retained.push(range.clone());
        let edits = self.edits;
        let first = edits.partition_point(|(from, end, _)| {
            end.get() < range.start || (end.get() == range.start && from < end)
        });
        let mut at = range.start;
        for (from, to, replacement) in edits[first..]
            .iter()
            .take_while(|(from, _, _)| from.get() < range.end)
        {
            self.copy_raw(at..from.get());
            self.flat_replacement(from.get()..to.get(), replacement);
            at = to.get();
        }
        self.copy_raw(at..range.end);
    }

    // Presentation retains physical line breaks as source slices, leaving flat replacements.
    fn flat_replacement(
        &mut self,
        range: Range<usize>,
        replacement: &'a crate::format::layout::pieces::Replacement,
    ) {
        let first = self.position();
        let original = self.source_line(range.start);
        self.origin.get_or_insert(original);
        self.anchor.get_or_insert(range.start);
        match replacement.piece() {
            Piece::Text(text) => self.text(text),
            Piece::Spaces(count) => self.spaces(count),
        }
        self.sources.push(SourceSite {
            range,
            position: first,
            columns: SourceColumns::Replaced {
                column: self.column,
                byte_column: self.bytes,
            },
        });
    }

    fn copy_raw(&mut self, range: Range<usize>) {
        if range.is_empty() {
            self.source(range);
            return;
        }
        let mut at = range.start;
        for (_, content, ending) in
            split_lines_preserve_endings(&self.input.text()[range])
        {
            self.source(at..at + content.len());
            if !ending.is_empty() {
                let original = self.source_line(at);
                self.flush(ending, original);
            }
            at += content.len() + ending.len();
        }
    }

    fn token(&mut self, symbol: Symbol, anchor: usize, width: usize) {
        self.chunks.push(Chunk::Generated(Placed {
            symbol,
            anchor,
            first: self.position(),
            last: Position {
                line: self.lines.len(),
                column: self.column + width,
                byte_column: self.bytes + width,
            },
        }));
        let origin = self.source_line(anchor);
        self.origin.get_or_insert(origin);
        self.anchor.get_or_insert(anchor);
    }

    fn structure(&mut self, structure: Structure, plan: &Plan) {
        let (node, width, closing) = match structure {
            Structure::Start { node, form, .. } => {
                self.changed.push((plan.span(node).start.get(), form));
                self.nodes.push((node, form));
                (node, usize::from(matches!(form, Form::Flow)), false)
            }
            Structure::End { node, form, .. } => {
                (node, usize::from(matches!(form, Form::Flow)), true)
            }
            Structure::Entry(node) | Structure::Value(node) => (node, 1, false),
            Structure::Key(node) => {
                self.keys.push(plan.span(node).end.get());
                (node, 0, false)
            }
        };
        let span = plan.span(node);
        self.token(
            Symbol::Structure(structure),
            if closing {
                span.end.get()
            } else {
                span.start.get()
            },
            width,
        );
    }

    fn flush(&mut self, ending: &'a str, fallback: usize) {
        let origin = self.origin.take().unwrap_or(fallback);
        let at =
            self.starts[origin.min(self.starts.len() - 1)].min(self.input.text().len());
        let anchor = self.anchor.take().unwrap_or(at);
        let mut parts = std::mem::take(&mut self.parts);
        let body = if let [RetainedPart::Source(text)] = parts.as_slice() {
            Some(*text)
        } else {
            None
        };
        let bridge = if parts.is_empty() || body.is_some() {
            None
        } else {
            Some(Box::new(Bridge {
                start: anchor,
                end: anchor,
                retained_characters: 0,
                prefix_column: 0,
                end_position: None,
                joined: false,
                tight_join: false,
                parts: Some(std::mem::take(&mut parts)),
            }))
        };
        self.lines.push(VirtualLine {
            origin,
            prefix: self.prefix,
            body: body.unwrap_or(&self.input.text()[at..at]),
            tail: "",
            ending,
            bom: self.bom,
            bridge,
        });
        parts.clear();
        self.parts = parts;
        self.prefix = 0;
        self.column = 0;
        self.bytes = 0;
        self.bom = false;
    }
}
