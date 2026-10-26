use std::borrow::Cow;
use std::ops::RangeInclusive;

use super::{
    Analyzer, Config, Gap, ID, Kind, Mode, Shift,
    input::{GapInitial, Initial},
};
use crate::directives::Directives;
use crate::format::layout::{VirtualLine, lines as virtual_lines};
use crate::rules::hyphens;
use crate::rules::support::line_syntax::buffer_newline;
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
    pub(crate) kept_dash_lines: Vec<(usize, usize)>,
    /// The spacing edits the dash shapes dropped or inserted, which a refused document
    /// reverts.
    pub(crate) reshaped: Vec<crate::rules::support::token_spacing::SpaceEdit>,
    consistent: Option<bool>,
    /// Whether `trailing-spaces` empties whitespace-only lines.
    pub(crate) trimmed: bool,
    /// The start of each scalar the plan restyles out of double quotes.
    pub(crate) restyled: Vec<usize>,
}

impl<'a> Layout<'a> {
    pub(crate) fn collapse_scalars<'b>(
        &mut self,
        edits: impl IntoIterator<Item = &'b crate::format::layout::PlanEdit>,
    ) -> bool {
        let source = self.input.text();
        let starts = self.input.byte_line_starts();
        let mut collapses = Vec::new();
        let mut compatible = true;
        for (start, end, text) in edits {
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
            let body_start =
                self.lines[first].body.as_ptr() as usize - source.as_ptr() as usize;
            let body_end = self.lines[last].body.as_ptr() as usize
                - source.as_ptr() as usize
                + self.lines[last].body.len();
            let end_line = starts.partition_point(|&at| at <= end.get()) - 1;
            let prefix = self.source_starts[self.lines[first].origin];
            collapses.push((
                first,
                last,
                Box::new(crate::format::layout::Bridge {
                    start: start.get(),
                    end: end.get(),
                    characters: text.char_count(),
                    prefix_column: source[prefix..start.get()].chars().count(),
                    // BOM-normalized columns retain the byte-derived tail width.
                    end_position: matches!(self.source_starts, Cow::Borrowed(_)).then(
                        || {
                            (
                                end_line,
                                source[starts[end_line]..end.get()].chars().count(),
                            )
                        },
                    ),
                }),
            ));
            compatible &= !text.piece().chars().any(|ch| matches!(ch, '\n' | '\r'))
                && first <= last
                && self.lines[first].tail.is_empty()
                && self.lines[last].tail.is_empty()
                && body_start <= start.get()
                && start.get() <= body_start + self.lines[first].body.len()
                && end.get() <= body_end;
        }
        if collapses.is_empty() {
            return true;
        }
        if !compatible || collapses.windows(2).any(|pair| pair[0].1 >= pair[1].0) {
            return false;
        }
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
                line.body = &source[body_start..start];
                line.tail = &source[end..body_end];
                line.ending = ending;
                line.bridge = Some(bridge);
            }
            self.lines.push(line);
        }
        self.reindex();
        true
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
        Self::with_shapes(input, cfg, false)
    }

    pub(crate) fn keep_refused(
        &mut self,
        refused: &[Option<Cause>],
        edits: &[crate::format::layout::PlanEdit],
    ) -> bool {
        let starts = self.starts.clone();
        let frozen =
            |line| refused[starts.partition_point(|&at| at <= line) - 1].is_some();
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
        }))
    }

    pub(crate) fn frozen(&self, refused: &[Option<Cause>], line: usize) -> bool {
        refused[self.starts.partition_point(|&at| at <= line) - 1].is_some()
    }

    pub(crate) fn freeze_targets(&self, targets: &mut Targets) -> Vec<Refusal> {
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
        super::check_with_analysis(self.input, &cfg.with_consistent_spaces())
            .iter()
            .all(|problem| {
                refused
                    .iter()
                    .any(|item| item.lines.contains(&problem.line))
            })
    }

    pub(crate) fn format(input: Analysis<'a>, cfg: &Config) -> Self {
        Self::with_shapes(input, cfg, true)
    }

    fn with_shapes(input: Analysis<'a>, cfg: &Config, streaming: bool) -> Self {
        let mut starts = input.parsed().document_starts.clone();
        let directives = Directives::parse_with_analysis(input);
        let source_starts = if streaming && !input.text().contains('\u{feff}') {
            Cow::Borrowed(input.byte_line_starts())
        } else {
            Cow::Owned(super::normalized_line_starts(input))
        };
        let mut lines = virtual_lines(input, &source_starts);
        let parsed = input.parsed().valid && !starts.is_empty();
        if parsed {
            starts[0] = 0;
        } else {
            starts.clear();
        }
        let kept_dash_lines = if parsed && streaming {
            cfg.dash_on_own_line.map_or_else(Vec::new, |own_line| {
                // A one-column join stands in for the join after the re-indent moves
                // the mapping `spaces` from its dash: none at width 1, and past width 2
                // only where `hyphens` may trim the gap that leaves.
                let spaced = |at: [usize; 2]| match cfg.spaces {
                    super::SpacesSetting::Fixed(1) => false,
                    super::SpacesSetting::Fixed(2) => true,
                    _ => !at
                        .iter()
                        .any(|&line| directives.is_disabled(hyphens::ID, line + 1)),
                };
                reshape_events(input, &mut lines, own_line, &spaced)
            })
        } else if parsed {
            reshape(
                input,
                &source_starts,
                &mut lines,
                cfg.dash_on_own_line,
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
            kept_dash_lines,
            reshaped: Vec::new(),
            consistent: None,
            trimmed: false,
            restyled: Vec::new(),
        };
        layout.reindex();
        layout
    }

    fn reindent_snapshot(
        &mut self,
        cfg: &Config,
    ) -> (Reindented, TextVersion<'static>) {
        let input = self.input;
        let buffer = input.text();
        if !self.parsed {
            return (
                Reindented {
                    text: buffer.to_string(),
                    refused: Vec::new(),
                    kept_dash_lines: Vec::new(),
                },
                TextVersion::new(buffer.to_string()),
            );
        }
        let original_line_count = input.line_count();
        let Targets {
            deltas,
            gaps,
            mut refused,
            ..
        } = self.targets(cfg, &[], false);
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
        if cfg.require_consistent {
            let verified = if output.analysis().text() == buffer {
                input
            } else {
                output.analysis()
            };
            refuse_inconsistent(verified, cfg, &mut refused);
            let plan = output.analysis().plan(render(&refused));
            output
                .apply(plan)
                .expect("consistency plan belongs to the current epoch");
        }
        (
            self.result(output.to_string(), &refused, original_line_count),
            output,
        )
    }

    pub(crate) fn targets(
        &mut self,
        cfg: &Config,
        edits: &[crate::format::layout::PlanEdit],
        folding: bool,
    ) -> Targets {
        let line_starts = self.line_starts();
        let mut analyzer = Analyzer::new(
            Initial::new(self.input.text(), &line_starts).projected(&self.lines),
            cfg,
            Mode::Target,
        );
        analyzer.scalar_owners = folding.then(Vec::new);
        let initial = GapInitial::new(&self.lines);
        super::walk(self.projected(edits), |token, previous, next, after| {
            let gap = analyzer.gap_request(token, next).and_then(|request| {
                let spaces = initial.after(request.line(), request.column());
                request.resolve(spaces)
            });
            analyzer.advance(token, previous, next, after, gap);
        });
        let deltas = settle(&self.lines, &analyzer.shifts);
        let refused = self.refusals(
            &analyzer.gaps,
            &analyzer.diagnostics,
            (&analyzer.block_tails, &deltas),
            edits,
        );
        Targets {
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
    ) -> impl Iterator<Item = super::Token> + 'b {
        let line_count = self.input.line_count();
        let projection =
            crate::format::layout::projection::Projection::new(self.input, edits);
        super::normalized_tokens(self.input, &self.source_starts).map(
            move |mut token| {
                let line = self.project_line(token.line, token.start);
                let end_line = if token.end_line >= line_count {
                    self.lines.len()
                } else {
                    self.project_line(token.end_line, token.end)
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
            },
        )
    }

    /// Each token on its virtual line, at the column the layout `targets` emits it at.
    fn targeted<'b>(
        &'b self,
        targets: &'b Targets,
        edits: &'b [crate::format::layout::PlanEdit],
    ) -> impl Iterator<Item = super::Token> + 'b {
        let column = move |line: usize, column: usize| {
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
        };
        self.projected(edits).map(move |mut token| {
            token.column = column(token.line, token.column);
            token.end_column = column(token.end_line, token.end_column);
            token
        })
    }

    /// The consistent-width violations of the layout `targets` emits, on virtual lines.
    pub(crate) fn target_violations(
        &self,
        cfg: &Config,
        targets: &Targets,
        edits: &[crate::format::layout::PlanEdit],
    ) -> Vec<super::Violation> {
        let line_starts = self.line_starts();
        let mut check = Analyzer::new(
            Initial::new(self.input.text(), &line_starts),
            &cfg.with_consistent_spaces(),
            Mode::Check,
        );
        check.run(self.targeted(targets, edits));
        check.diagnostics
    }

    /// Each kept dash's 1-based virtual line and column in the layout `targets` emits.
    pub(crate) fn kept_dash_targets(
        &self,
        targets: &Targets,
        edits: &[crate::format::layout::PlanEdit],
    ) -> Vec<(usize, usize)> {
        let starts = self.input.byte_line_starts();
        let dashes: Vec<usize> = self
            .kept_dash_lines
            .iter()
            .map(|&(line, column)| starts[line - 1] + column - 1)
            .collect();
        self.targeted(targets, edits)
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
    pub(crate) fn split_starts(&self) -> Vec<usize> {
        self.shapes().split
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
                .filter(|line| {
                    line.bridge.is_none()
                        && !line.tail.is_empty()
                        && !line.tail.starts_with(' ')
                })
                .map(|line| line.tail.as_ptr() as usize - source)
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
        passes: &'b crate::fix::Passes<'_>,
        facts: &'b std::cell::RefCell<crate::format::layout::facts::Builder<'a>>,
        spacing: &'b mut Vec<crate::rules::support::token_spacing::SpaceSite>,
    ) -> impl Iterator<Item = (super::Token, super::Token)> + 'b {
        use crate::format::layout::projection::Projection;
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
            let projected = self.project_line(token.line, token.start);
            let end = if token.end_line >= input.line_count() {
                self.lines.len()
            } else {
                self.project_line(token.end_line, token.end)
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

    pub(crate) fn project_line(&self, original: usize, byte: usize) -> usize {
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
            && byte >= bridge.end
        {
            let source_line = self.lines[index].origin;
            let prefix =
                projection.column(source_line, bridge.start, bridge.prefix_column);
            let prefix = extra.map_or(prefix, |extra| {
                extra.column(source_line, bridge.start, prefix)
            });
            let tail = bridge
                .end_position
                .filter(|(line, _)| *line == original)
                .and_then(|(_, end_column)| column.checked_sub(end_column))
                .unwrap_or_else(|| self.input.text()[bridge.end..byte].chars().count());
            return prefix + bridge.characters + tail;
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
        let mut refused: Vec<Option<Cause>> = vec![None; self.starts.len()];
        let mut refuse = |line: usize, cause: Cause| {
            let document = self.starts.partition_point(|&start| start <= line) - 1;
            refused[document].get_or_insert(cause);
        };
        for line in 0..self.input.line_count() {
            if directives.is_disabled(ID, line + 1) {
                refuse(line, Cause::Disabled);
            }
        }
        for gap in gaps {
            if directives.is_disabled(gap.rule(), self.lines[gap.line()].origin + 1) {
                refuse(self.lines[gap.line()].origin, Cause::Disabled);
            }
        }
        let mut protected = None;
        let source = self.input.text().as_ptr() as usize;
        // An empty tail is a literal outside the input.
        let emitted = |text: &str| {
            if text.is_empty() {
                String::new()
            } else {
                edited(text, text.as_ptr() as usize - source, edits)
            }
        };
        for virtual_line in &self.lines {
            // A tab counts only as the layout emits it, once the edits have run. The
            // edits do not hold trailing-spaces yet: where it runs, a whitespace-only line
            // outside the scalars it protects is blank.
            if !virtual_line.body.contains('\t') && !virtual_line.tail.contains('\t') {
                continue;
            }
            let line = virtual_line.origin;
            // A joined mapping's tab stays indentation, as on its own line.
            let tail = emitted(virtual_line.tail);
            let content = emitted(virtual_line.body) + &tail;
            if (tab_in_indentation(&content) || tab_in_indentation(&tail))
                && (!content.trim_start_matches([' ', '\t']).is_empty()
                    || !self.trimmed
                    || directives
                        .is_disabled(crate::rules::trailing_spaces::ID, line + 1)
                    || protected
                        .get_or_insert_with(|| {
                            crate::rules::trailing_spaces::protected_lines(
                                self.input,
                                |start| self.restyled.contains(&start),
                            )
                            .map(|(lines, _)| lines)
                            .unwrap_or_default()
                        })
                        .contains(&(line + 1)))
            {
                refuse(line, Cause::Tab);
            }
        }
        for problem in diagnostics {
            refuse(self.lines[problem.line - 1].origin, Cause::Unfollowable);
        }
        for &(line, body, content) in block_tails {
            let Some(&delta) = deltas.get(line) else {
                continue;
            };
            let virtual_line = &self.lines[line];
            let text = virtual_line.body.trim_start();
            let column = super::to_isize(
                virtual_line.prefix + virtual_line.body.len() - text.len(),
            );
            let content = content.map_or(column + 1, super::to_isize);
            if text.starts_with('#') && column + delta >= content + body {
                refuse(virtual_line.origin, Cause::Changed);
            }
        }
        refused
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

pub(crate) struct Targets {
    pub(crate) deltas: Vec<isize>,
    pub(crate) gaps: Vec<Gap>,
    pub(crate) refused: Vec<Option<Cause>>,
    pub(crate) scalar_owners: Vec<(usize, usize)>,
    /// The last line a token leads or a scalar carries.
    pub(crate) last_token: Option<usize>,
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
    ) -> Targets {
        let deltas = settle(&layout.lines, &self.shifts);
        Targets {
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

fn shape(
    entry: &super::Token,
    start: &super::Token,
    own_line: bool,
    lines: &[VirtualLine<'_>],
) -> Option<(usize, Option<usize>, usize)> {
    (entry.kind == Kind::BlockEntry && start.kind == Kind::BlockMappingStart)
        .then(|| {
            if own_line && start.line == entry.line {
                Some((entry.line, Some(start.column), entry.column))
            } else if !own_line
                && start.line == entry.line + 1
                && start.column > entry.column + 1
                && lines[entry.line].prefix + lines[entry.line].body.trim_end().len()
                    == entry.column + 1
            {
                Some((entry.line, None, entry.column))
            } else {
                None
            }
        })
        .flatten()
}

fn refuse_inconsistent(
    input: Analysis<'_>,
    cfg: &Config,
    refused: &mut [Option<Cause>],
) {
    let mut starts = input.parsed().document_starts.clone();
    starts[0] = 0;
    for violation in super::check_with_analysis(input, &cfg.with_consistent_spaces()) {
        let document = starts.partition_point(|&start| start < violation.line) - 1;
        refused[document].get_or_insert(Cause::Inconsistent);
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
    directives: &Directives,
) -> Vec<(usize, usize)> {
    let buffer = input.text();
    let mut shapes = Vec::new();
    let mut kept = Vec::new();
    if let Some(own_line) = dash_on_own_line {
        let commented: std::collections::HashSet<_> = input
            .tokens()
            .iter(input.text())
            .filter_map(|(span, kind)| {
                matches!(kind, crate::text_version::lexical::Kind::Comment { .. })
                    .then_some(span.start.line() - 1)
            })
            .collect();
        let mut tokens = super::normalized_tokens(input, source_starts).peekable();
        while let Some(entry) = tokens.next() {
            let Some(&start) = tokens.peek() else {
                break;
            };
            if entry.kind != Kind::BlockEntry
                || start.kind != Kind::BlockMappingStart
                || directives.is_disabled(hyphens::ID, entry.line + 1)
                || directives.is_disabled(hyphens::ID, start.line + 1)
            {
                continue;
            }
            if let Some((line, split, dash)) = shape(&entry, &start, own_line, lines) {
                if split.is_some() && commented.contains(&line) {
                    kept.push((line + 1, dash + 1));
                } else {
                    shapes.push((line, split, dash));
                }
            }
        }
    }
    if shapes.is_empty() {
        return kept;
    }
    apply_shapes(buffer, lines, shapes);
    kept
}

fn apply_shapes<'a>(
    buffer: &'a str,
    lines: &mut Vec<VirtualLine<'a>>,
    shapes: Vec<(usize, Option<usize>, usize)>,
) {
    let original = std::mem::take(lines);
    lines.reserve(original.len() + shapes.len());
    let mut physical = original.into_iter().enumerate();
    let mut shapes = shapes.into_iter().peekable();
    while let Some((index, mut current)) = physical.next() {
        let shape = shapes.next_if(|(line, _, _)| *line == index);
        if let Some((_, split, dash)) = shape {
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
                if current.ending.is_empty() {
                    current.ending = buffer_newline(buffer);
                }
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
    spaced: &dyn Fn([usize; 2]) -> bool,
) -> Vec<(usize, usize)> {
    use ryl_yaml_parser::{Event, StructureStyle};
    let mut stack = Vec::new();
    let mut shapes = Vec::new();
    let mut comments = std::collections::HashSet::new();
    for (event, span) in input.event_iter() {
        match event {
            Event::Comment(..) => {
                comments.insert(span.start.line() - 1);
            }
            Event::MappingStart(style, anchor, tag) => {
                if stack.last() == Some(&true)
                    && style == StructureStyle::Block
                    && anchor == 0
                    && tag.is_none()
                {
                    shapes.extend(event_shape(input, span, own_line));
                }
                stack.push(false);
            }
            Event::SequenceStart(..) => stack.push(true),
            Event::MappingEnd | Event::SequenceEnd => {
                stack.pop();
            }
            _ => {}
        }
    }
    // A join `spaced` refuses refuses its document's re-indent, and so every join in it.
    let document = |line| {
        input
            .parsed()
            .document_starts
            .partition_point(|&at| at <= line)
    };
    let refused: Vec<_> = shapes
        .iter()
        .filter(|&&(line, _, _, tight)| tight && !spaced([line, line + 1]))
        .map(|&(line, ..)| document(line))
        .collect();
    let mut kept = Vec::new();
    let shapes: Vec<_> = shapes
        .into_iter()
        .filter(|&(line, split, dash, tight)| {
            let keep = split.is_some() && comments.contains(&line);
            if keep {
                kept.push((line + 1, dash + 1));
            }
            !(keep || tight && refused.contains(&document(line)))
        })
        .map(|(line, split, dash, _)| (line, split, dash))
        .collect();
    if !shapes.is_empty() {
        apply_shapes(input.text(), lines, shapes);
    }
    kept
}

fn event_shape(
    input: Analysis<'_>,
    span: ryl_yaml_parser::Span,
    own_line: bool,
) -> Option<(usize, Option<usize>, usize, bool)> {
    let line = span.start.line() - 1;
    let at = crate::rules::support::span_utils::marker_byte_offset(span.start).get();
    let prefix = &input.text()[input.byte_line_starts()[line]..at];
    let prefix = prefix.strip_prefix('\u{feff}').unwrap_or(prefix);
    let inline = prefix
        .trim_end_matches([' ', '\t'])
        .strip_suffix('-')
        .map(str::len);
    let entry = inline.map(|column| (line, column)).or_else(|| {
        line.checked_sub(1).and_then(|above| {
            input
                .line_content(above)
                .trim_start_matches('\u{feff}')
                .trim_end_matches([' ', '\t'])
                .strip_suffix('-')
                .filter(|prefix| !prefix.contains('#'))
                .map(|prefix| (above, prefix.len()))
        })
    });
    let (entry, dash) = entry?;
    if own_line && entry == line {
        Some((entry, Some(span.start.col()), dash, false))
    } else if !own_line && entry + 1 == line && span.start.col() > dash {
        Some((entry, None, dash, span.start.col() == dash + 1))
    } else {
        None
    }
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
) -> String {
    let end = start + content.len();
    let mut text = String::new();
    let mut at = start;
    for (from, to, replacement) in
        &edits[edits.partition_point(|(_, to, _)| to.get() < start)..]
    {
        let (from, to) = (from.get(), to.get());
        if from > end {
            break;
        }
        text.push_str(&content[at - start..from.max(at) - start]);
        text.extend(replacement.piece().chars());
        at = to.min(end);
    }
    text.push_str(&content[at - start..]);
    text
}

fn tab_in_indentation(content: &str) -> bool {
    let rest = content.trim_start_matches(' ');
    rest.starts_with('\t')
        || ["-", "?", ":"].iter().any(|indicator| {
            rest.strip_prefix(indicator).is_some_and(|after| {
                after.starts_with('\t')
                    && !after.trim_start_matches([' ', '\t']).is_empty()
            })
        })
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
            None if content.comment_column().is_some() => {
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
