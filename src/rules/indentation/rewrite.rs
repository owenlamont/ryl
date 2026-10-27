use std::borrow::Cow;
use std::cell::OnceCell;
use std::ops::RangeInclusive;

use super::{
    Analyzer, Config, Gap, Kind, Mode, Shift,
    input::{GapInitial, Initial, MappingSite},
    settlement::{Content, Refusals},
    shape,
};
use crate::directives::Directives;
use crate::format::layout::{VirtualLine, lines as virtual_lines};
use crate::rules::hyphens;
use crate::text_version::{Analysis, TextVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reindented {
    pub text: String,
    /// Each document left as it was.
    pub refused: Vec<Refusal>,
    /// The 1-based line and column of each `-` whose mapping stays on its line because a
    /// comment follows, though `cfg` asks for it on the next.
    pub kept_dash_lines: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The document's 1-based lines.
    pub lines: RangeInclusive<usize>,
    pub cause: Cause,
}

/// Why a document is left as it was, in the order they are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// An inline directive turns off `indentation`, or the rule owning a gap it would close.
    Disabled,
    Tab,
    /// The analyzer met a token it cannot place.
    Unfollowable,
    Inconsistent,
    /// The re-indented document parses to different events.
    Changed,
}

/// `buffer` with each line moved to where [`super::check`] under `cfg` expects it, one
/// document at a time, the spaces after a `-`, `?` or `:` that opens content closed to
/// one, and dash-line mappings joined or broken as `cfg` asks. A document is left
/// byte-identical for each [`Cause`].
#[must_use]
pub fn reindent(buffer: &str, cfg: &Config) -> Reindented {
    reindent_with_analysis(TextVersion::new(buffer).analysis(), cfg).0
}

pub(crate) fn reindent_with_analysis(
    input: Analysis<'_>,
    cfg: &Config,
) -> (Reindented, TextVersion<'static>) {
    Layout::new(input, cfg).reindent_snapshot(cfg)
}

pub(crate) struct Layout<'a> {
    input: Analysis<'a>,
    parsed: bool,
    directives: Directives,
    starts: Vec<usize>,
    source_starts: Cow<'a, [usize]>,
    pub(crate) lines: Vec<VirtualLine<'a>>,
    pub(crate) shaped: bool,
    projection: Option<Vec<usize>>,
    converted: Option<Box<crate::format::layout::frame::Frame>>,
    baseline: Option<Vec<VirtualLine<'a>>>,
    converted_shapes: Vec<shape::Recipe>,
    shape_rows: Option<shape::Rows>,
    pub(crate) kept_dash_lines: Vec<(usize, usize)>,
    /// The spacing edits the dash shapes dropped or inserted, which a refused document
    /// reverts.
    pub(crate) reshaped: Vec<crate::rules::support::token_spacing::SpaceEdit>,
    consistent: Option<bool>,
    /// Whether `trailing-spaces` empties whitespace-only lines.
    pub(crate) trimmed: bool,
    /// The start of each scalar the plan restyles out of double quotes.
    restyled: crate::rules::quoted_strings::Restyled,
    protection: OnceCell<Option<crate::rules::trailing_spaces::Protection>>,
}

pub(crate) struct CollectionFeasibility(Option<Vec<Option<Cause>>>);

impl CollectionFeasibility {
    pub(crate) fn observe<'a>(
        input: Analysis<'a>,
        cfg: &Config,
        consistent: bool,
        plan: &crate::rules::support::collection_style::Plan,
    ) -> (Self, crate::format::layout::frame::FrameRows<'a>) {
        let mut unshaped = *cfg;
        unshaped.dash_on_own_line = None;
        let (converted, lines) = crate::format::layout::frame::Frame::new(
            input,
            input.byte_line_starts(),
            plan,
        );
        let mut layout = Layout::converted(input, &unshaped, converted, lines);
        let candidates = layout.retained_candidates(cfg);
        let feasibility = Self(candidates.map(|mut candidates| {
            let styles = crate::rules::quoted_strings::Styles::default();
            if cfg.require_consistent || consistent {
                let violations =
                    layout.target_violations(cfg, &candidates, &[], &styles);
                layout.refuse_violated(&violations, &mut candidates.refused);
            }
            candidates.refused
        }));
        (
            feasibility,
            (
                *layout
                    .converted
                    .expect("the feasibility frame was installed"),
                layout.lines,
            ),
        )
    }

    pub(crate) fn refuses(&self, index: usize) -> bool {
        self.0
            .as_ref()
            .is_some_and(|documents| documents[index].is_some())
    }
}

pub(crate) struct Width(usize);

impl Width {
    pub(crate) const fn characters(&self) -> usize {
        self.0
    }
}

pub(crate) struct CollectionGeometry<'a> {
    source: Analysis<'a>,
    config: Option<Config>,
    layout: OnceCell<(Layout<'a>, Targets)>,
    positions: Option<Vec<(usize, usize)>>,
}

impl<'a> CollectionGeometry<'a> {
    pub(crate) const fn new(source: Analysis<'a>, config: Option<Config>) -> Self {
        Self {
            source,
            config,
            layout: OnceCell::new(),
            positions: None,
        }
    }

    pub(crate) fn selected(
        source: Analysis<'a>,
        mut positions: Vec<(usize, usize)>,
    ) -> Self {
        positions.sort_unstable_by_key(|&(at, _)| at);
        Self {
            source,
            config: None,
            layout: OnceCell::new(),
            positions: Some(positions),
        }
    }

    pub(crate) fn width(
        &self,
        at: crate::rules::support::span_utils::BytePos,
        characters: usize,
    ) -> Width {
        if let Some(positions) = &self.positions {
            let index = positions
                .binary_search_by_key(&at.get(), |&(at, _)| at)
                .expect("selected geometry covers each collection site");
            return Width(positions[index].1 + characters);
        }
        let source = self.source.text();
        let start = source[..at.get()]
            .rfind(['\n', '\r'])
            .map_or(0, |byte| byte + 1);
        let width = source[start..at.get()].chars().count() + characters;
        Width(self.config.map_or(width, |config| {
            let (layout, targets) = self.layout.get_or_init(|| {
                let layout = Layout::new(self.source, &config);
                let candidates = layout.targets(
                    &config,
                    &[],
                    &crate::rules::quoted_strings::Styles::default(),
                    false,
                );
                let (targets, _) = candidates.settle(&layout);
                (layout, targets)
            });
            let origin = self
                .source
                .byte_line_starts()
                .partition_point(|&byte| byte <= at.get())
                - 1;
            let line = layout.project_line(origin, at.get());
            width.saturating_add_signed(targets.deltas()[line])
        }))
    }
}

impl<'a> Layout<'a> {
    pub(crate) fn collection_form(
        &self,
        node: usize,
    ) -> Option<crate::rules::support::collection_style::Form> {
        self.converted
            .as_ref()
            .and_then(|frame| frame.collection_form(node))
    }

    pub(crate) fn keeps_source_edit(
        &self,
        start: crate::rules::support::span_utils::BytePos,
        end: crate::rules::support::span_utils::BytePos,
    ) -> bool {
        self.converted
            .as_ref()
            .is_none_or(|frame| frame.retains_edit(start, end))
    }

    pub(crate) fn is_converted(&self) -> bool {
        self.converted.is_some()
    }

    pub(crate) fn restyle(&mut self, styles: crate::rules::quoted_strings::Restyled) {
        self.restyled = styles;
        self.protection.take();
    }

    pub(crate) fn protection(
        &self,
    ) -> Option<&crate::rules::trailing_spaces::Protection> {
        self.protection
            .get_or_init(|| {
                crate::rules::trailing_spaces::protection(self.input, &self.restyled)
            })
            .as_ref()
    }

    pub(crate) fn column(&self, index: usize, delta: isize) -> super::Column {
        let line = &self.lines[index];
        let retained = super::to_isize(
            line.prefix + line.body.len() - line.body.trim_start().len(),
        );
        super::Column {
            origin: line.origin,
            anchor: line.source_anchor(self.input.text()),
            retained,
            current: retained + delta,
        }
    }

    pub(crate) fn collapse_scalars<'b>(
        &mut self,
        edits: impl IntoIterator<Item = &'b crate::format::layout::PlanEdit>,
    ) {
        let source = self.input.text();
        let starts = self.input.byte_line_starts();
        let mut collapses: Vec<(usize, usize, Box<crate::format::layout::Bridge<'a>>)> =
            Vec::new();
        for (start, end, _) in edits {
            if !source[start.get()..end.get()].contains(['\n', '\r']) {
                continue;
            }
            let first = self.project_line(
                starts.partition_point(|&at| at <= start.get()) - 1,
                start.get(),
            );
            let last = self.project_line(
                starts.partition_point(|&at| at < end.get()) - 1,
                end.get(),
            );
            let line = &self.lines[first];
            let joined = !line.tail.is_empty()
                && line.tail.as_ptr() as usize - source.as_ptr() as usize
                    <= start.get();
            let prefix = self.source_starts[line.origin];
            let bridge_start = if joined {
                line.tail.as_ptr() as usize - source.as_ptr() as usize
            } else {
                start.get()
            };
            let end_line = starts.partition_point(|&at| at <= end.get()) - 1;
            let end_position =
                matches!(self.source_starts, Cow::Borrowed(_)).then(|| {
                    (
                        end_line,
                        source[starts[end_line]..end.get()].chars().count(),
                    )
                });
            if let Some((_, previous, bridge)) = collapses.last_mut()
                && *previous == first
            {
                *previous = last;
                bridge.retained_characters +=
                    source[bridge.end..end.get()].chars().count();
                bridge.end = end.get();
                bridge.end_position = end_position;
            } else {
                collapses.push((
                    first,
                    last,
                    Box::new(crate::format::layout::Bridge {
                        start: bridge_start,
                        end: end.get(),
                        retained_characters: source[bridge_start..end.get()]
                            .chars()
                            .count(),
                        prefix_column: if joined {
                            line.prefix + line.body.chars().count()
                        } else {
                            source[prefix..start.get()].chars().count()
                        },
                        end_position,
                        joined,
                        tight_join: joined && !line.tail.starts_with(' '),
                        parts: None,
                    }),
                ));
            }
        }
        if collapses.is_empty() {
            return;
        }
        self.apply_collapses(collapses);
    }

    fn apply_collapses(
        &mut self,
        collapses: Vec<(usize, usize, Box<crate::format::layout::Bridge<'a>>)>,
    ) {
        let source = self.input.text();
        let original = std::mem::take(&mut self.lines);
        self.lines.reserve(original.len());
        let mut physical = original.into_iter().enumerate();
        let mut collapses = collapses.into_iter().peekable();
        while let Some((index, mut line)) = physical.next() {
            if let Some((_, last, bridge)) =
                collapses.next_if(|(first, _, _)| *first == index)
            {
                let (start, end) = (bridge.start, bridge.end);
                let body_start = line.body.as_ptr() as usize - source.as_ptr() as usize;
                let mut ending = line.ending;
                let mut body_end = body_start + line.body.len();
                for (_, later) in physical.by_ref().take(last - index) {
                    ending = later.ending;
                    body_end = later.body.as_ptr() as usize - source.as_ptr() as usize
                        + later.body.len();
                }
                if !bridge.joined {
                    line.body = &source[body_start..start];
                }
                line.tail = &source[end..body_end];
                line.ending = ending;
                line.bridge = Some(bridge);
            }
            self.lines.push(line);
        }
        self.reindex();
    }

    pub(crate) fn consistent(&self, cfg: &Config) -> bool {
        if let Some(consistent) = self.consistent {
            return consistent;
        }
        let mut analyzer = Analyzer::new(
            Initial::new(self.input.text(), &self.source_starts),
            &cfg.with_consistent_spaces(),
            Mode::Check,
        );
        analyzer.run(super::normalized_tokens(self.input, &self.source_starts));
        analyzer.diagnostics.is_empty()
    }
    pub(crate) fn new(input: Analysis<'a>, cfg: &Config) -> Self {
        Self::with_shapes(input, cfg, false, None)
    }

    pub(crate) fn converted(
        input: Analysis<'a>,
        cfg: &Config,
        frame: crate::format::layout::frame::Frame,
        lines: Vec<VirtualLine<'a>>,
    ) -> Self {
        let mut unshaped = *cfg;
        unshaped.dash_on_own_line = None;
        let mut layout = Self::with_shapes(input, &unshaped, true, Some(lines));
        layout.converted = Some(Box::new(frame));
        layout.shaped = true;
        if let Some(own_line) = cfg.dash_on_own_line {
            let tokens = super::projected::tokens(
                layout.converted.as_ref().expect("the frame was installed"),
                input,
                super::normalized_tokens(input, &layout.source_starts),
            );
            let mut tokens = tokens.peekable();
            let mut candidates = Vec::new();
            while let Some(entry) = tokens.next() {
                let Some(start) = tokens.peek() else { break };
                candidates.extend(shape::from_tokens(
                    &entry,
                    start,
                    &hyphens::dash_gap(layout.lines.get(entry.line)),
                    own_line,
                    &layout.directives,
                    true,
                ));
            }
            let mut comments = crate::rules::comments::RetainedComments::default();
            for (event, span) in input.event_iter() {
                if matches!(event, ryl_yaml_parser::Event::Comment(..)) {
                    let position = layout
                        .converted
                        .as_ref()
                        .expect("the frame was installed")
                        .source_position(
                            input,
                            crate::rules::support::span_utils::marker_byte_offset(
                                span.start,
                            )
                            .get(),
                            false,
                        );
                    comments.record_line(position.line);
                }
            }
            let (recipes, kept) = shape::settle(
                candidates,
                &comments,
                &shape::TightJoins::new(cfg.spaces, &layout.directives),
                &layout.starts,
            );
            layout.converted_shapes = recipes;
            layout.kept_dash_lines = kept;
        }
        if !layout.converted_shapes.is_empty() {
            layout.baseline = Some(layout.lines.clone());
            layout.shape_rows = Some(shape::apply_projected(
                input.text(),
                &mut layout.lines,
                layout.converted_shapes.iter().cloned(),
            ));
            layout.reindex();
        }
        layout
    }

    pub(crate) fn keep_refused(
        &mut self,
        refused: &[Option<Cause>],
        edits: &[crate::format::layout::PlanEdit],
    ) {
        if self.converted.is_some() && self.baseline.is_none() {
            return;
        }
        let starts = self.starts.clone();
        let frozen =
            |line| refused[starts.partition_point(|&at| at <= line) - 1].is_some();
        if let Some(baseline) = &self.baseline {
            self.lines.clone_from(baseline);
            let recipes = self
                .converted_shapes
                .iter()
                .filter(|recipe| !frozen(baseline[recipe.line()].origin))
                .cloned();
            self.shape_rows = Some(shape::apply_projected(
                self.input.text(),
                &mut self.lines,
                recipes,
            ));
            self.reindex();
            return;
        }
        let mut previous = std::mem::take(&mut self.lines).into_iter().peekable();
        for (origin, content, ending) in self.input.lines() {
            if frozen(origin) {
                while previous.peek().is_some_and(|line| line.origin <= origin) {
                    previous.next();
                }
                self.lines.push(VirtualLine::new(
                    origin,
                    content,
                    ending,
                    self.source_starts[origin] > self.input.byte_line_starts()[origin],
                ));
            } else {
                while previous.peek().is_some_and(|line| line.origin == origin) {
                    self.lines
                        .push(previous.next().expect("the line was inspected"));
                }
            }
        }
        self.shaped = self.lines.len() != self.input.line_count()
            || self.lines.iter().any(|line| !line.tail.is_empty());
        self.reindex();
        let input = self.input;
        self.collapse_scalars(edits.iter().filter(|(start, _, _)| {
            frozen(
                input
                    .byte_line_starts()
                    .partition_point(|&at| at <= start.get())
                    - 1,
            )
        }));
    }

    pub(crate) fn frozen(&self, refused: &[Option<Cause>], line: usize) -> bool {
        refused[self.starts.partition_point(|&at| at <= line) - 1].is_some()
    }

    fn freeze_candidates(&self, targets: &mut Candidates) -> Vec<Refusal> {
        for (index, line) in self.lines.iter().enumerate() {
            if self.frozen(&targets.refused, line.origin) {
                targets.deltas[index] = 0;
            }
        }
        let refused = &targets.refused;
        targets
            .gaps
            .retain(|gap| !self.frozen(refused, self.lines[gap.line()].origin));
        self.result(String::new(), &targets.refused, self.input.line_count())
            .refused
    }

    /// Refuses each document holding one of `violations` as [`Cause::Inconsistent`], and
    /// whether that refused one more.
    pub(crate) fn refuse_violated(
        &self,
        violations: &[super::Violation],
        refused: &mut [Option<Cause>],
    ) -> bool {
        let mut more = false;
        for violation in violations {
            let cause = &mut refused[self
                .starts
                .partition_point(|&at| at <= self.lines[violation.line - 1].origin)
                - 1];
            more |= cause.is_none();
            cause.get_or_insert(Cause::Inconsistent);
        }
        more
    }

    pub(crate) fn consistent_except(&self, cfg: &Config, refused: &[Refusal]) -> bool {
        if refused.is_empty() {
            return self.consistent(cfg);
        }
        let mut analyzer = Analyzer::new(
            Initial::new(self.input.text(), &self.source_starts),
            &cfg.with_consistent_spaces(),
            Mode::Check,
        );
        analyzer.run(super::normalized_tokens(self.input, &self.source_starts));
        analyzer.diagnostics.iter().all(|problem| {
            refused
                .iter()
                .any(|item| item.lines.contains(&problem.line))
        })
    }

    pub(crate) fn format(input: Analysis<'a>, cfg: &Config) -> Self {
        Self::with_shapes(input, cfg, true, None)
    }

    fn with_shapes(
        input: Analysis<'a>,
        cfg: &Config,
        streaming: bool,
        supplied: Option<Vec<VirtualLine<'a>>>,
    ) -> Self {
        let mut starts = input.parsed().document_starts.clone();
        let directives = Directives::parse_with_analysis(input);
        let source_starts = if streaming && !input.text().contains('\u{feff}') {
            Cow::Borrowed(input.byte_line_starts())
        } else {
            Cow::Owned(super::normalized_line_starts(input))
        };
        let mut lines =
            supplied.unwrap_or_else(|| virtual_lines(input, &source_starts));
        let parsed = input.parsed().valid && !starts.is_empty();
        if parsed {
            starts[0] = 0;
        } else {
            starts.clear();
        }
        let kept_dash_lines = if parsed && streaming {
            cfg.dash_on_own_line.map_or_else(Vec::new, |own_line| {
                let joins = shape::TightJoins::new(cfg.spaces, &directives);
                reshape_events(input, &mut lines, own_line, &joins)
            })
        } else if parsed {
            reshape(
                input,
                &source_starts,
                &mut lines,
                cfg.dash_on_own_line,
                cfg.spaces,
                &directives,
            )
        } else {
            Vec::new()
        };
        let shaped = lines.len() != input.line_count()
            || lines.iter().any(|line| !line.tail.is_empty());
        let mut layout = Self {
            input,
            parsed,
            directives,
            starts,
            source_starts,
            lines,
            shaped,
            projection: None,
            converted: None,
            baseline: None,
            converted_shapes: Vec::new(),
            shape_rows: None,
            kept_dash_lines,
            reshaped: Vec::new(),
            consistent: None,
            trimmed: false,
            restyled: crate::rules::quoted_strings::Restyled::default(),
            protection: OnceCell::new(),
        };
        layout.reindex();
        layout
    }

    fn retained_candidates(&mut self, cfg: &Config) -> Option<Candidates> {
        self.parsed.then(|| {
            self.targets(
                cfg,
                &[],
                &crate::rules::quoted_strings::Styles::default(),
                false,
            )
        })
    }

    fn reindent_snapshot(
        &mut self,
        cfg: &Config,
    ) -> (Reindented, TextVersion<'static>) {
        let input = self.input;
        let buffer = input.text();
        let Some(mut candidates) = self.retained_candidates(cfg) else {
            return (
                Reindented {
                    text: buffer.to_string(),
                    refused: Vec::new(),
                    kept_dash_lines: Vec::new(),
                },
                TextVersion::new(buffer.to_string()),
            );
        };
        let original_line_count = input.line_count();
        let styles = crate::rules::quoted_strings::Styles::default();
        if cfg.require_consistent {
            self.settle_consistency(cfg, &mut candidates, &styles);
        }
        let Candidates {
            deltas,
            gaps,
            refused,
            ..
        } = candidates;
        let lines = &self.lines;
        let starts = &self.starts;
        let end_of = |index: usize| {
            starts
                .get(index + 1)
                .copied()
                .unwrap_or(original_line_count)
        };
        let render = |refused: &[Option<Cause>]| {
            let mut text = String::with_capacity(buffer.len());
            for (index, cause) in refused.iter().enumerate() {
                if cause.is_some() {
                    let original = input
                        .lines_from(starts[index])
                        .take(end_of(index) - starts[index]);
                    text.extend(
                        original.flat_map(|(_, content, ending)| [content, ending]),
                    );
                    continue;
                }
                let first = lines.partition_point(|line| line.origin < starts[index]);
                let last = lines.partition_point(|line| line.origin < end_of(index));
                for line in first..last {
                    crate::format::layout::emit_line(
                        &mut text,
                        buffer,
                        &lines[line],
                        deltas[line],
                        &gaps[gaps.partition_point(|gap| gap.line() < line)
                            ..gaps.partition_point(|gap| gap.line() <= line)],
                    );
                }
            }
            text
        };
        let mut output = TextVersion::new(render(&refused));
        output.inherit_event_capacity(input);
        debug_assert!(
            output.analysis().text() == buffer
                || input.significant_events_match(output.analysis()),
            "re-indentation changed a document's events"
        );
        (
            self.result(output.to_string(), &refused, original_line_count),
            output,
        )
    }

    fn settle_consistency(
        &self,
        cfg: &Config,
        candidates: &mut Candidates,
        styles: &crate::rules::quoted_strings::Styles,
    ) {
        let violations = self.target_violations(cfg, candidates, &[], styles);
        self.refuse_violated(&violations, &mut candidates.refused);
    }

    pub(crate) fn targets(
        &self,
        cfg: &Config,
        edits: &[crate::format::layout::PlanEdit],
        styles: &crate::rules::quoted_strings::Styles,
        folding: bool,
    ) -> Candidates {
        let tokens = super::normalized_tokens(self.input, &self.source_starts);
        self.targets_from(cfg, edits, styles, folding, tokens)
    }

    pub(crate) fn prepared_targets(
        &self,
        cfg: &Config,
        edits: &[crate::format::layout::PlanEdit],
        styles: &crate::rules::quoted_strings::Styles,
        folding: bool,
    ) -> Result<Candidates, crate::format::layout::Fallback> {
        if self.input.layout_facts().is_some() {
            return Ok(self.targets(cfg, edits, styles, folding));
        }
        let facts = std::cell::RefCell::new(
            crate::format::layout::facts::Builder::new(self.input, folding),
        );
        let scanner = self
            .input
            .tokens()
            .iter(self.input.text())
            .inspect(|(span, kind)| facts.borrow_mut().push(*span, kind));
        let tokens = super::normalized_tokens_from(
            self.input.text(),
            &self.source_starts,
            scanner,
        );
        let targets = self.targets_from(cfg, edits, styles, folding, tokens);
        facts.into_inner().finish().map(|facts| {
            self.input.seed_layout_facts(facts);
            targets
        })
    }

    fn targets_from(
        &self,
        cfg: &Config,
        edits: &[crate::format::layout::PlanEdit],
        styles: &crate::rules::quoted_strings::Styles,
        folding: bool,
        tokens: impl Iterator<Item = super::Token>,
    ) -> Candidates {
        let line_starts = self.line_starts();
        let mut analyzer = Analyzer::new(
            Initial::new(self.input.text(), &line_starts).projected(&self.lines),
            cfg,
            Mode::Target,
        );
        analyzer.scalar_owners = folding.then(Vec::new);
        let initial = GapInitial::new(&self.lines);
        super::walk(
            self.projected_from(tokens, edits, styles),
            |token, previous, next, after| {
                let gap = analyzer.gap_request(token, next).and_then(|request| {
                    let spaces = initial.after(request.line(), request.column());
                    request.resolve(spaces)
                });
                analyzer.advance(token, previous, next, after, gap);
            },
        );
        let deltas = settle(&self.lines, &analyzer.shifts);
        let refused = self.refusals(
            &analyzer.gaps,
            &analyzer.diagnostics,
            (&analyzer.block_tails, &deltas),
            edits,
        );
        Candidates {
            deltas,
            gaps: analyzer.gaps,
            refused,
            scalar_owners: analyzer.scalar_owners.unwrap_or_default(),
            last_token: analyzer.shifts.iter().rposition(Option::is_some),
        }
    }

    /// Each token on its virtual line, at its column after `edits`.
    fn projected<'b>(
        &'b self,
        edits: &'b [crate::format::layout::PlanEdit],
        styles: &'b crate::rules::quoted_strings::Styles,
    ) -> impl Iterator<Item = super::Token> + 'b {
        let tokens = super::normalized_tokens(self.input, &self.source_starts);
        self.projected_from(tokens, edits, styles)
    }

    fn projected_from<'b>(
        &'b self,
        tokens: impl Iterator<Item = super::Token> + 'b,
        edits: &'b [crate::format::layout::PlanEdit],
        styles: &'b crate::rules::quoted_strings::Styles,
    ) -> impl Iterator<Item = super::Token> + 'b {
        let line_count = self.input.line_count();
        let projection =
            crate::format::layout::projection::Projection::new(self.input, edits);
        let mut styles = styles.cursor();
        let tokens = if let Some(converted) = &self.converted {
            rayon::iter::Either::Right(super::projected::tokens(
                converted, self.input, tokens,
            ))
        } else {
            rayon::iter::Either::Left(tokens)
        };
        tokens.map(move |mut token| {
            token.restyle(&mut styles);
            if let Some(rows) = &self.shape_rows {
                let first = rows.position(crate::format::layout::frame::Position {
                    line: token.line,
                    column: token.column,
                    byte_column: token.column,
                });
                let last = rows.position(crate::format::layout::frame::Position {
                    line: token.end_line,
                    column: token.end_column,
                    byte_column: token.end_column,
                });
                token.line = first.line;
                token.column = first.column;
                token.end_line = last.line;
                token.end_column = last.column;
                return token;
            }
            if self.converted.is_some() {
                return token;
            }
            let line = self.source_line(token.line, token.start);
            let end_line = if token.end_line >= line_count {
                self.lines.len()
            } else {
                self.source_line(token.end_line, token.end)
            };
            if !edits.is_empty() {
                token.column = self.project_column(
                    (&projection, None),
                    line,
                    token.line,
                    token.start,
                    token.column,
                );
                token.end_column = self.project_column(
                    (&projection, None),
                    end_line,
                    token.end_line,
                    token.end,
                    token.end_column,
                );
            }
            token.line = line;
            token.end_line = end_line;
            token
        })
    }

    /// Each token on its virtual line, at the column the layout `targets` emits it at.
    fn targeted<'b>(
        &'b self,
        targets: &'b Candidates,
        edits: &'b [crate::format::layout::PlanEdit],
        styles: &'b crate::rules::quoted_strings::Styles,
    ) -> impl Iterator<Item = super::Token> + 'b {
        self.projected(edits, styles).map(move |mut token| {
            token.column = self.target_column(targets, token.line, token.column);
            token.end_column =
                self.target_column(targets, token.end_line, token.end_column);
            token
        })
    }

    fn target_column(&self, targets: &Candidates, line: usize, column: usize) -> usize {
        if self.lines.get(line).is_none_or(|virtual_line| {
            self.frozen(&targets.refused, virtual_line.origin)
        }) {
            return column;
        }
        let closed: usize = targets.gaps
            [targets.gaps.partition_point(|gap| gap.line() < line)..]
            .iter()
            .take_while(|gap| gap.line() == line)
            .filter(|gap| gap.column() + 1 < column)
            .map(|gap| gap.removed())
            .sum();
        column.saturating_add_signed(targets.deltas[line]) - closed
    }

    /// The consistent-width violations of the layout `targets` emits, on virtual lines.
    pub(crate) fn target_violations(
        &self,
        cfg: &Config,
        targets: &Candidates,
        edits: &[crate::format::layout::PlanEdit],
        styles: &crate::rules::quoted_strings::Styles,
    ) -> Vec<super::Violation> {
        let line_starts = self.line_starts();
        let mut check = Analyzer::new(
            Initial::new(self.input.text(), &line_starts),
            &cfg.with_consistent_spaces(),
            Mode::Check,
        );
        check.run(self.targeted(targets, edits, styles));
        check.diagnostics
    }

    /// Each kept dash's 1-based virtual line and column in the layout `targets` emits.
    pub(crate) fn kept_dash_targets(
        &self,
        targets: &Targets,
        edits: &[crate::format::layout::PlanEdit],
        styles: &crate::rules::quoted_strings::Styles,
    ) -> Vec<(usize, usize)> {
        if self.converted.is_some() {
            return self
                .kept_dash_lines
                .iter()
                .map(|&(line, column)| {
                    let position = crate::format::layout::frame::Position {
                        line: line - 1,
                        column: column - 1,
                        byte_column: column - 1,
                    };
                    let position = self
                        .shape_rows
                        .as_ref()
                        .map_or(position, |rows| rows.position(position));
                    (
                        position.line + 1,
                        self.target_column(&targets.0, position.line, position.column)
                            + 1,
                    )
                })
                .collect();
        }
        let starts = self.input.byte_line_starts();
        let dashes: Vec<usize> = self
            .kept_dash_lines
            .iter()
            .map(|&(line, column)| starts[line - 1] + column - 1)
            .collect();
        self.targeted(&targets.0, edits, styles)
            .filter(|token| {
                token.kind == Kind::BlockEntry && dashes.contains(&token.start)
            })
            .map(|token| (token.line + 1, token.column + 1))
            .collect()
    }

    pub(crate) fn stream_targets(
        &mut self,
        cfg: &Config,
        edits: &[crate::format::layout::PlanEdit],
        styles: &crate::rules::quoted_strings::Styles,
        passes: &crate::fix::Passes<'_>,
    ) -> Result<
        (
            StreamTargets,
            Vec<crate::rules::support::token_spacing::SpaceSite>,
        ),
        crate::format::layout::Fallback,
    > {
        use crate::format::layout::facts::Builder;
        let input = self.input;
        let folding = passes.line_length.is_some();
        let facts = std::cell::RefCell::new(Builder::new(input, folding));
        let mut spacing = Vec::new();
        let starts = self.line_starts();
        let mut analyzer = Analyzer::new(
            Initial::new(input.text(), &starts).projected(&self.lines),
            cfg,
            Mode::Target,
        );
        let consistent_cfg = cfg.with_consistent_spaces();
        let mut consistency =
            (edits.is_empty() && cfg.requires_consistency()).then(|| {
                Analyzer::new(
                    Initial::new(input.text(), &self.source_starts),
                    &consistent_cfg,
                    Mode::Check,
                )
            });
        analyzer.scalar_owners = folding.then(Vec::new);
        let reshaped;
        {
            let mut shapes = self
                .shaped
                .then(|| self.shapes())
                .filter(|shapes| !shapes.split.is_empty() || !shapes.joined.is_empty());
            let tokens = self.projected_tokens(
                (edits, shapes.as_mut()),
                styles,
                passes,
                &facts,
                &mut spacing,
            );
            let initial = GapInitial::new(&self.lines);
            super::walk(tokens, |(raw, token), previous, next, after| {
                let gap = analyzer
                    .gap_request(token, next.map(|pair| &pair.1))
                    .and_then(|request| {
                        let spaces = initial.after(request.line(), request.column());
                        request.resolve(spaces)
                    });
                analyzer.advance(
                    token,
                    previous.map(|pair| &pair.1),
                    next.map(|pair| &pair.1),
                    after.map(|pair| &pair.1),
                    gap,
                );
                if let Some(check) = &mut consistency {
                    check.advance(
                        raw,
                        previous.map(|pair| &pair.0),
                        next.map(|pair| &pair.0),
                        after.map(|pair| &pair.0),
                        None,
                    );
                }
            });
            reshaped = shapes.map_or_else(Vec::new, |shapes| shapes.reshaped);
        }
        let Analyzer {
            shifts,
            gaps,
            diagnostics,
            scalar_owners,
            block_tails,
            ..
        } = analyzer;
        let consistent = consistency.map(|check| check.diagnostics.is_empty());
        facts.into_inner().finish().map(|facts| {
            input.seed_layout_facts(facts);
            self.consistent = consistent;
            self.reshaped = reshaped;
            (
                StreamTargets {
                    shifts,
                    gaps,
                    diagnostics,
                    scalar_owners: scalar_owners.unwrap_or_default(),
                    block_tails,
                },
                spacing,
            )
        })
    }

    /// The byte each mapping a split moves onto its own line starts at.
    pub(crate) fn split_targets(&self) -> super::SplitTargets {
        super::SplitTargets::new(self.shapes().split)
    }

    pub(crate) fn joined_starts(&self) -> Vec<usize> {
        self.shapes().joined
    }

    fn shapes(&self) -> Shapes {
        let source = self.input.text().as_ptr() as usize;
        Shapes {
            split: self
                .lines
                .windows(2)
                .filter(|pair| pair[0].origin == pair[1].origin)
                .map(|pair| pair[1].body.as_ptr() as usize - source)
                .collect(),
            joined: self
                .lines
                .iter()
                .filter_map(|line| {
                    if let Some(bridge) = &line.bridge {
                        bridge.tight_join.then_some(bridge.start)
                    } else {
                        (!line.tail.is_empty() && !line.tail.starts_with(' '))
                            .then(|| line.tail.as_ptr() as usize - source)
                    }
                })
                .collect(),
            next: 0,
            reshaped: Vec::new(),
        }
    }

    fn line_starts(&self) -> Cow<'_, [usize]> {
        if self.projection.is_none() && self.source_starts.len() == self.lines.len() + 1
        {
            return Cow::Borrowed(&self.source_starts);
        }
        let mut starts: Vec<_> = self
            .lines
            .iter()
            .map(|line| self.source_starts[line.origin])
            .collect();
        starts.push(*self.source_starts.last().expect("scanner has an EOF line"));
        Cow::Owned(starts)
    }

    fn projected_tokens<'b>(
        &'b self,
        (edits, mut shapes): (
            &'b [crate::format::layout::PlanEdit],
            Option<&'b mut Shapes>,
        ),
        styles: &'b crate::rules::quoted_strings::Styles,
        passes: &'b crate::fix::Passes<'_>,
        facts: &'b std::cell::RefCell<crate::format::layout::facts::Builder<'a>>,
        spacing: &'b mut Vec<crate::rules::support::token_spacing::SpaceSite>,
    ) -> impl Iterator<Item = (super::Token, super::Token)> + 'b {
        use crate::format::layout::projection::Projection;
        let mut styles = styles.cursor();
        let input = self.input;
        let projection = Projection::new(input, edits);
        let mut gaps = Projection::incremental();
        let scanner = input
            .tokens()
            .iter(input.text())
            .inspect(move |(span, kind)| {
                facts.borrow_mut().push(*span, kind);
            });
        let mut tokens =
            super::normalized_tokens_from(input.text(), &self.source_starts, scanner)
                .peekable();
        let mut line = std::collections::VecDeque::new();
        std::iter::from_fn(move || {
            if line.is_empty() {
                let first = tokens.next()?;
                let original = first.line;
                line.push_back(first);
                while tokens.peek().is_some_and(|token| {
                    token.line == original || token.kind == Kind::BlockEnd
                }) {
                    line.push_back(tokens.next().expect("peeked token exists"));
                }
                let mut next = facts.borrow_mut().sites.edits_through(
                    original + 1,
                    passes.colons == Some(crate::rules::colons::Config::format()),
                    passes.hyphens == Some(crate::rules::hyphens::Config::format()),
                );
                if let Some(shapes) = shapes.as_deref_mut() {
                    next = shapes.respace(next, &line);
                }
                gaps.append(input, &next);
                spacing.extend(next);
            }
            let original = line.pop_front().expect("a source line has a token");
            let mut token = original;
            token.restyle(&mut styles);
            let projected = self.source_line(token.line, token.start);
            let end = if token.end_line >= input.line_count() {
                self.lines.len()
            } else {
                self.source_line(token.end_line, token.end)
            };
            if !edits.is_empty() || !spacing.is_empty() {
                token.column = self.project_column(
                    (&projection, Some(&gaps)),
                    projected,
                    token.line,
                    token.start,
                    token.column,
                );
                token.end_column = self.project_column(
                    (&projection, Some(&gaps)),
                    end,
                    token.end_line,
                    token.end,
                    token.end_column,
                );
            }
            token.line = projected;
            token.end_line = end;
            Some((original, token))
        })
    }

    fn result(
        &self,
        text: String,
        refused: &[Option<Cause>],
        line_count: usize,
    ) -> Reindented {
        Reindented {
            text,
            kept_dash_lines: self.kept_dash_lines.clone(),
            refused: refused
                .iter()
                .enumerate()
                .filter_map(|(index, cause)| {
                    cause.map(|cause| Refusal {
                        lines: self.starts[index] + 1
                            ..=self
                                .starts
                                .get(index + 1)
                                .copied()
                                .unwrap_or(line_count),
                        cause,
                    })
                })
                .collect(),
        }
    }

    pub(crate) fn converted_foldables(
        &self,
    ) -> Option<Vec<crate::rules::line_length::fold::FoldableScalar>> {
        self.converted.as_ref()?;
        let mut foldables = crate::rules::line_length::fold::Foldables::default();
        let styles = crate::rules::quoted_strings::Styles::default();
        for token in self.projected(&[], &styles) {
            use crate::text_version::lexical::Kind as Lexical;
            let kind = match token.kind {
                Kind::Scalar { style, .. } => {
                    foldables.scalar(token.start..token.end, style);
                    continue;
                }
                Kind::StreamBoundary | Kind::Marker => Lexical::StreamStart,
                Kind::BlockSequenceStart => Lexical::BlockSequenceStart,
                Kind::BlockMappingStart => Lexical::BlockMappingStart,
                Kind::BlockEnd => Lexical::BlockEnd,
                Kind::FlowSequenceStart => Lexical::FlowSequenceStart,
                Kind::FlowSequenceEnd => Lexical::FlowSequenceEnd,
                Kind::FlowMappingStart => Lexical::FlowMappingStart,
                Kind::FlowMappingEnd => Lexical::FlowMappingEnd,
                Kind::BlockEntry => Lexical::BlockEntry,
                Kind::Value => Lexical::Value,
                Kind::Property => Lexical::Anchor,
                _ => Lexical::Other,
            };
            foldables.push_site(
                &kind,
                token.line,
                token.column,
                token.start..token.end,
            );
        }
        Some(foldables.finish())
    }

    pub(crate) fn converted_position(
        &self,
        byte: usize,
        end: bool,
    ) -> Option<crate::format::layout::frame::Position> {
        let position = self
            .converted
            .as_ref()?
            .source_position(self.input, byte, end);
        Some(
            self.shape_rows
                .as_ref()
                .map_or(position, |rows| rows.position(position)),
        )
    }

    pub(crate) fn closed_source_gaps(
        &self,
        targets: &Targets,
    ) -> Vec<crate::format::layout::PlanEdit> {
        let mut edits: Vec<_> = targets
            .gaps()
            .iter()
            .filter_map(|gap| {
                let from = gap.column() + 2;
                self.lines[gap.line()]
                    .source_span(self.input.text(), from..from + gap.removed())
                    .map(|(start, end)| {
                        (start, end, crate::format::layout::Replacement::Spaces(0))
                    })
            })
            .collect();
        edits.extend(self.reshaped.iter().filter(|(from, to, _)| from < to).map(
            |&(from, to, spaces)| {
                (from, to, crate::format::layout::Replacement::Spaces(spaces))
            },
        ));
        if let Some(baseline) = &self.baseline {
            for recipe in &self.converted_shapes {
                let (line, split, dash) = recipe.clone().into_parts();
                let Some(column) = split else { continue };
                if !targets.refused().is_empty()
                    && self.frozen(targets.refused(), baseline[line].origin)
                {
                    continue;
                }
                if let Some((from, to)) =
                    baseline[line].source_span(self.input.text(), dash + 1..column)
                {
                    edits.push((
                        from,
                        to,
                        crate::format::layout::Replacement::Spaces(1),
                    ));
                }
            }
        } else if self.converted.is_none() {
            for end in self.shapes().split {
                let start =
                    self.input.text()[..end].trim_end_matches([' ', '\t']).len();
                edits.push((
                    crate::rules::support::span_utils::BytePos::new(start),
                    crate::rules::support::span_utils::BytePos::new(end),
                    crate::format::layout::Replacement::Spaces(1),
                ));
            }
        }
        edits
    }

    pub(crate) fn collection_position(
        &self,
        byte: usize,
        targets: &Candidates,
        projection: &crate::format::layout::projection::Projection,
    ) -> crate::format::layout::frame::Position {
        let original = self
            .input
            .byte_line_starts()
            .partition_point(|&at| at <= byte)
            .saturating_sub(1);
        let mut position = self.converted.as_ref().map_or_else(
            || crate::format::layout::frame::Position {
                line: self.project_line(original, byte),
                byte_column: byte - self.input.byte_line_starts()[original],
                column: self.input.text()
                    [self.input.byte_line_starts()[original]..byte]
                    .chars()
                    .count(),
            },
            |frame| frame.source_position(self.input, byte, false),
        );
        if self.converted.is_none() {
            position.column = self.project_column(
                (projection, None),
                position.line,
                original,
                byte,
                position.column,
            );
        }
        if let Some(rows) = &self.shape_rows {
            position = rows.position(position);
        }
        let delta = targets.deltas[position.line];
        let removed: usize = targets.gaps[targets
            .gaps
            .partition_point(|gap| gap.line() < position.line)..]
            .iter()
            .take_while(|gap| gap.line() == position.line)
            .filter(|gap| gap.column() + 1 < position.column)
            .map(|gap| gap.removed())
            .sum();
        position.column = position
            .column
            .saturating_add_signed(delta)
            .saturating_sub(removed);
        position
    }

    #[inline]
    pub(crate) fn project_line(&self, original: usize, byte: usize) -> usize {
        if let Some(converted) = &self.converted {
            return self.converted_line(converted, byte);
        }
        self.source_line(original, byte)
    }

    #[inline]
    fn source_line(&self, original: usize, byte: usize) -> usize {
        let source = self.input.text().as_ptr() as usize;
        let Some(projection) = &self.projection else {
            return original.min(self.lines.len().saturating_sub(1));
        };
        let first = projection[original];
        first
            + usize::from(self.lines.get(first + 1).is_some_and(|line| {
                line.origin == original && line.body.as_ptr() as usize - source <= byte
            }))
    }

    // Keep the source-only coordinate lookup small enough to inline in token walks.
    #[inline(never)]
    fn converted_line(
        &self,
        frame: &crate::format::layout::frame::Frame,
        byte: usize,
    ) -> usize {
        let position = frame.source_position(
            self.input,
            self.input.text().floor_char_boundary(byte),
            false,
        );
        self.shape_rows
            .as_ref()
            .map_or(position, |rows| rows.position(position))
            .line
            .min(self.lines.len().saturating_sub(1))
    }

    fn reindex(&mut self) {
        if self
            .lines
            .iter()
            .enumerate()
            .all(|(index, line)| line.origin == index)
        {
            self.projection = None;
            return;
        }
        let mut projection = vec![0; self.source_starts.len()];
        let (mut next, mut previous) = (0, 0);
        for (index, line) in self.lines.iter().enumerate() {
            while next < line.origin {
                projection[next] = previous;
                next += 1;
            }
            if next == line.origin {
                projection[next] = index;
                next += 1;
            }
            previous = index;
        }
        projection[next..].fill(previous);
        self.projection = Some(projection);
    }

    fn project_column(
        &self,
        projections: (
            &crate::format::layout::projection::Projection,
            Option<&crate::format::layout::projection::Projection>,
        ),
        index: usize,
        original: usize,
        byte: usize,
        column: usize,
    ) -> usize {
        let (projection, extra) = projections;
        if let Some(bridge) = self
            .lines
            .get(index)
            .and_then(|line| line.bridge.as_deref())
            && byte >= bridge.start
        {
            let source_line = self.lines[index].origin;
            let prefix =
                projection.column(source_line, bridge.start, bridge.prefix_column);
            let prefix = extra.map_or(prefix, |extra| {
                extra.column(source_line, bridge.start, prefix)
            });
            let prefix = if bridge.tight_join {
                prefix.saturating_add_signed(
                    projection.delta_at(bridge.start)
                        + extra.map_or(0, |extra| extra.delta_at(bridge.start)),
                )
            } else {
                prefix
            };
            let retained = if byte >= bridge.end {
                bridge.retained_characters
                    + bridge
                        .end_position
                        .filter(|(line, _)| *line == original)
                        .and_then(|(_, end)| column.checked_sub(end))
                        .unwrap_or_else(|| {
                            self.input.text()[bridge.end..byte].chars().count()
                        })
            } else {
                self.input.text()[bridge.start..byte].chars().count()
            };
            let selected = projection.characters(bridge.start, byte, retained);
            return prefix
                + extra.map_or(selected, |extra| {
                    extra.characters(bridge.start, byte, selected)
                });
        }
        let project = |byte, column| {
            let column = projection.column(original, byte, column);
            extra.map_or(column, |extra| extra.column(original, byte, column))
        };
        // A line split off a source line starts afresh, so edits before the split do not
        // move its tokens.
        let split = index.checked_sub(1).is_some_and(|previous| {
            self.lines
                .get(index)
                .is_some_and(|line| self.lines[previous].origin == line.origin)
        });
        let start = split.then(|| {
            self.lines[index].body.as_ptr() as usize
                - self.input.text().as_ptr() as usize
        });
        start
            .filter(|&start| start <= byte)
            .and_then(|start| {
                let from = column
                    .checked_sub(self.input.text()[start..byte].chars().count())?;
                (project(byte, column) + from).checked_sub(project(start, from))
            })
            .unwrap_or_else(|| project(byte, column))
    }

    /// Each document's first [`Cause`]; [`Cause::Changed`] only where a block scalar's
    /// first trailing comment could move into its body.
    fn refusals(
        &self,
        gaps: &[Gap],
        diagnostics: &[super::Violation],
        (block_tails, deltas): (&[super::BlockTail], &[isize]),
        edits: &[crate::format::layout::PlanEdit],
    ) -> Vec<Option<Cause>> {
        if self.starts.is_empty() {
            return Vec::new();
        }
        let directives = &self.directives;
        let mut refused = Refusals::new(&self.starts, directives);
        for line in 0..self.input.line_count() {
            refused.line(line);
        }
        for &gap in gaps {
            refused.gap(gap, self.lines[gap.line()].origin);
        }
        let source = self.input.text().as_ptr() as usize;
        // An empty tail is a literal outside the input.
        let emitted = |text: &str| {
            if text.is_empty() {
                Content::default()
            } else {
                edited(text, text.as_ptr() as usize - source, edits)
            }
        };
        for virtual_line in &self.lines {
            // A tab counts only as the layout emits it, once the edits have run. The
            // edits do not hold trailing-spaces yet: where it runs, a whitespace-only line
            // outside the scalars it protects is blank.
            if !virtual_line.body.contains('\t')
                && !virtual_line.tail.contains('\t')
                && !virtual_line
                    .parts()
                    .into_iter()
                    .flatten()
                    .any(|part| part.piece().chars().any(|ch| ch == '\t'))
            {
                continue;
            }
            let line = virtual_line.origin;
            // A joined mapping's tab stays indentation, as on its own line.
            let tail = emitted(virtual_line.tail);
            let mut content = emitted(virtual_line.body);
            for part in virtual_line.parts().into_iter().flatten() {
                content.extend(part.piece().chars());
            }
            if let Some(candidate) = content.tab_candidate(tail) {
                let protection = refused
                    .needs_protection(line, candidate, self.trimmed)
                    .then(|| self.protection())
                    .flatten();
                refused.tab(line, candidate, self.trimmed, protection);
            }
        }
        for problem in diagnostics {
            refused.unfollowable(self.lines[problem.line - 1].origin);
        }
        for &(line, body, content) in block_tails {
            let Some(&delta) = deltas.get(line) else {
                continue;
            };
            let virtual_line = &self.lines[line];
            refused.block_tail(
                virtual_line.origin,
                crate::rules::comments_indentation::retained_line(virtual_line),
                delta,
                body,
                content,
            );
        }
        refused.finish()
    }
}

/// The bytes where dash shapes start a line: each mapping a split moves onto its own line,
/// and each joined straight after its `-`.
struct Shapes {
    split: Vec<usize>,
    joined: Vec<usize>,
    next: usize,
    reshaped: Vec<crate::rules::support::token_spacing::SpaceEdit>,
}

impl Shapes {
    /// Drops the gap a split dash no longer has, and gives the next join through `last`
    /// its space.
    #[cold]
    #[inline(never)]
    fn respace(
        &mut self,
        mut sites: Vec<crate::rules::support::token_spacing::SpaceSite>,
        line: &std::collections::VecDeque<super::Token>,
    ) -> Vec<crate::rules::support::token_spacing::SpaceSite> {
        sites.retain(|site| {
            let kept = self.split.binary_search(&site.edit.1.get()).is_err();
            if !kept {
                self.reshaped.push(site.edit);
            }
            kept
        });
        let last = line.back().map_or(0, |token| token.start);
        if let Some(&at) = self.joined.get(self.next).filter(|&&at| at <= last) {
            self.next += 1;
            let at = crate::rules::support::span_utils::BytePos::new(at);
            self.reshaped.push((at, at, 1));
            sites.insert(
                0,
                crate::rules::support::token_spacing::SpaceSite {
                    indicator: crate::rules::support::token_spacing::Indicator::Dash,
                    edit: (at, at, 1),
                },
            );
        }
        sites
    }
}

pub(crate) struct Candidates {
    pub(crate) deltas: Vec<isize>,
    pub(crate) gaps: Vec<Gap>,
    pub(crate) refused: Vec<Option<Cause>>,
    pub(crate) scalar_owners: Vec<(usize, usize)>,
    /// The last line a token leads or a scalar carries.
    pub(crate) last_token: Option<usize>,
}

pub(crate) struct Targets(Candidates);

impl Candidates {
    pub(crate) fn settle(mut self, layout: &Layout<'_>) -> (Targets, Vec<Refusal>) {
        let refused = if self.refused.iter().any(Option::is_some) {
            layout.freeze_candidates(&mut self)
        } else {
            Vec::new()
        };
        (Targets(self), refused)
    }
}

impl Targets {
    pub(crate) const fn candidates(&self) -> &Candidates {
        &self.0
    }

    pub(crate) fn dedents_line(&self, line: usize) -> bool {
        self.0.deltas[line] < 0
    }

    pub(crate) fn deltas(&self) -> &[isize] {
        &self.0.deltas
    }

    pub(crate) fn gaps(&self) -> &[Gap] {
        &self.0.gaps
    }

    pub(crate) fn refused(&self) -> &[Option<Cause>] {
        &self.0.refused
    }

    pub(crate) fn scalar_owners(&self) -> &[(usize, usize)] {
        &self.0.scalar_owners
    }
}

pub(crate) struct StreamTargets {
    shifts: Vec<Option<Shift>>,
    gaps: Vec<Gap>,
    diagnostics: Vec<super::Violation>,
    scalar_owners: Vec<(usize, usize)>,
    block_tails: Vec<super::BlockTail>,
}

impl StreamTargets {
    pub(crate) fn finish(
        self,
        layout: &Layout<'_>,
        edits: &[crate::format::layout::PlanEdit],
    ) -> Candidates {
        let deltas = settle(&layout.lines, &self.shifts);
        Candidates {
            refused: layout.refusals(
                &self.gaps,
                &self.diagnostics,
                (&self.block_tails, &deltas),
                edits,
            ),
            deltas,
            gaps: self.gaps,
            scalar_owners: self.scalar_owners,
            last_token: self.shifts.iter().rposition(Option::is_some),
        }
    }
}

/// `buffer` with each block mapping in a block sequence joined onto its `-` line, or
/// broken onto the next, as `dash_on_own_line` asks, keeping every token's column; and the
/// line of `lines` each output line starts on. A dash line carrying a node property or a
/// comment, or one where `hyphens` is disabled, is left as it is.
fn reshape<'a>(
    input: Analysis<'a>,
    source_starts: &[usize],
    lines: &mut Vec<VirtualLine<'a>>,
    dash_on_own_line: Option<bool>,
    spaces: super::SpacesSetting,
    directives: &Directives,
) -> Vec<(usize, usize)> {
    let Some(own_line) = dash_on_own_line else {
        return Vec::new();
    };
    let mut comments = crate::rules::comments::RetainedComments::default();
    for (span, kind) in input.tokens().iter(input.text()) {
        if matches!(kind, crate::text_version::lexical::Kind::Comment { .. }) {
            comments.record(span);
        }
    }
    let mut candidates = Vec::new();
    let mut tokens = super::normalized_tokens(input, source_starts).peekable();
    while let Some(entry) = tokens.next() {
        let Some(start) = tokens.peek() else {
            break;
        };
        candidates.extend(shape::from_tokens(
            &entry,
            start,
            &hyphens::dash_gap(lines.get(entry.line)),
            own_line,
            directives,
            false,
        ));
    }
    let (recipes, kept) = shape::settle(
        candidates,
        &comments,
        &shape::TightJoins::new(spaces, directives),
        &input.parsed().document_starts,
    );
    if !recipes.is_empty() {
        apply_shapes(input.text(), lines, recipes);
    }
    kept
}

fn apply_shapes<'a>(
    buffer: &'a str,
    lines: &mut Vec<VirtualLine<'a>>,
    shapes: Vec<shape::Recipe>,
) {
    let original = std::mem::take(lines);
    lines.reserve(original.len() + shapes.len());
    let mut physical = original.into_iter().enumerate();
    let mut shapes = shapes.into_iter().peekable();
    while let Some((index, mut current)) = physical.next() {
        let shape = shapes.next_if(|recipe| recipe.line() == index);
        if let Some(recipe) = shape {
            let (_, split, dash) = recipe.into_parts();
            if let Some(at) = split {
                let next = VirtualLine {
                    origin: current.origin,
                    prefix: at,
                    body: &current.body[at - current.prefix..],
                    tail: "",
                    ending: current.ending,
                    bom: false,
                    bridge: None,
                };
                current.body = &current.body[..=dash - current.prefix];
                current.ending =
                    crate::format::layout::line_endings::split(buffer, current.ending)
                        .materialize();
                lines.push(current);
                lines.push(next);
                continue;
            }
            let (_, below) = physical
                .next()
                .expect("a join has a following mapping line");
            current.body = &current.body[..=dash - current.prefix];
            let end = below.body.as_ptr() as usize - buffer.as_ptr() as usize
                + below.body.len();
            let start = end - below.body.len() - below.prefix + dash + 1;
            current.tail = &buffer[start..end];
            current.ending = below.ending;
        }
        lines.push(current);
    }
}

fn reshape_events<'a>(
    input: Analysis<'a>,
    lines: &mut Vec<VirtualLine<'a>>,
    own_line: bool,
    joins: &shape::TightJoins<'_>,
) -> Vec<(usize, usize)> {
    use crate::rules::support::collection_style::RetainedContext;
    use crate::text_version::model::{Container, Mapping};
    use ryl_yaml_parser::Event;
    let mut stack = Vec::new();
    let mut candidates = Vec::new();
    let mut comments = crate::rules::comments::RetainedComments::default();
    for (event, span) in input.event_iter() {
        match event {
            Event::Comment(..) => comments.record(span),
            Event::MappingStart(style, anchor, tag) => {
                let model = Mapping::new(stack.last().copied(), anchor, tag.is_some());
                if let Some(admission) = shape::MappingAdmission::new(
                    &model,
                    RetainedContext::from_style(style),
                ) {
                    candidates.extend(shape::from_event(
                        admission,
                        MappingSite::observe(span.start),
                        &hyphens::entry_site(input, span.start),
                        own_line,
                    ));
                }
                stack.push(Container::Mapping);
            }
            Event::SequenceStart(..) => stack.push(Container::Sequence),
            Event::MappingEnd | Event::SequenceEnd => {
                stack.pop();
            }
            _ => {}
        }
    }
    let (recipes, kept) = shape::settle(
        candidates,
        &comments,
        joins,
        &input.parsed().document_starts,
    );
    if !recipes.is_empty() {
        apply_shapes(input.text(), lines, recipes);
    }
    kept
}

/// The re-indent `ryl format` applies, or `None` where nothing moves.
#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let text = reindent(buffer, cfg).text;
    (text != buffer).then_some(text)
}

/// `content`, which starts at byte `start`, with the `edits` that touch it applied; an
/// edit that crosses the line's ends writes its whole replacement at that end.
fn edited(
    content: &str,
    start: usize,
    edits: &[crate::format::layout::PlanEdit],
) -> Content {
    let end = start + content.len();
    let mut text = Content::default();
    let mut at = start;
    for (from, to, replacement) in
        &edits[edits.partition_point(|(_, to, _)| to.get() < start)..]
    {
        let (from, to) = (from.get(), to.get());
        if from > end {
            break;
        }
        text.extend(content[at - start..from.max(at) - start].chars());
        text.extend(replacement.piece().chars());
        at = to.min(end);
    }
    text.extend(content[at - start..].chars());
    text
}

/// Each line's shift, settling the lines no token or scalar leads: a whole-line comment
/// keeps its alignment with the token line after it, else the one before, else moves to
/// the next token line's column; anything else moves with the line before it.
fn settle(lines: &[VirtualLine<'_>], shifts: &[Option<Shift>]) -> Vec<isize> {
    let token = |line: usize| match shifts[line] {
        Some(Shift::Token { found, delta }) => Some((found, delta)),
        _ => None,
    };
    let mut following = (0..lines.len())
        .filter_map(|line| token(line).map(|target| (line, target)))
        .peekable();
    let mut deltas = Vec::with_capacity(lines.len());
    let mut previous = None;
    for (line, content) in lines.iter().enumerate() {
        while following.next_if(|(index, _)| *index <= line).is_some() {}
        let column = super::to_isize(
            content.prefix + content.body.len() - content.body.trim_start().len(),
        );
        let delta = match shifts[line] {
            Some(shift) => shift.delta(),
            None if crate::rules::comments_indentation::retained_line(content)
                .column()
                .is_some() =>
            {
                match (following.peek().map(|(_, target)| *target), previous) {
                    (Some((found, delta)), _) if found == column => delta,
                    (_, Some((found, delta))) if found == column => delta,
                    (Some((found, delta)), _) => found + delta - column,
                    (None, previous) => previous.map_or(0, |(_, delta)| delta),
                }
            }
            None => deltas.last().copied().unwrap_or(0),
        };
        deltas.push(delta.max(-column));
        previous = token(line).or(previous);
    }
    deltas
}

pub(crate) struct LineGeometry<'a> {
    anchor: crate::rules::support::span_utils::BytePos,
    line: &'a VirtualLine<'a>,
    delta: isize,
}

impl LineGeometry<'_> {
    pub(crate) const fn source_anchor(
        &self,
    ) -> crate::rules::support::span_utils::BytePos {
        self.anchor
    }

    pub(crate) fn empty(&self) -> bool {
        self.line.is_empty()
    }
    pub(crate) fn prefix(&self) -> usize {
        self.line.prefix.saturating_add_signed(self.delta) * usize::from(!self.empty())
    }
    pub(crate) const fn retained_prefix(&self) -> usize {
        self.line.prefix
    }
    pub(crate) const fn projected_prefix(&self) -> usize {
        self.line.prefix.saturating_add_signed(self.delta)
    }
    pub(crate) const fn origin(&self) -> usize {
        self.line.origin
    }
    pub(crate) const fn bom(&self) -> bool {
        self.line.bom
    }
    pub(crate) const fn has_body(&self) -> bool {
        !self.line.body.is_empty() || self.line.bridge.is_some()
    }
    pub(crate) const fn has_tail(&self) -> bool {
        !self.line.tail.is_empty()
    }
    pub(crate) const fn has_break(&self) -> bool {
        !self.line.ending.is_empty()
    }
    pub(crate) fn scalar_content(
        &self,
    ) -> crate::rules::quoted_strings::LineContent<'_> {
        crate::rules::quoted_strings::line_content(self.line.content())
    }
}

impl Layout<'_> {
    pub(crate) fn geometry(&self, index: usize, targets: &Targets) -> LineGeometry<'_> {
        LineGeometry {
            anchor: self.lines[index].source_anchor(self.input.text()),
            line: &self.lines[index],
            delta: targets.deltas()[index],
        }
    }
}
