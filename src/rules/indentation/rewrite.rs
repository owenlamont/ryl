use std::borrow::Cow;
use std::ops::RangeInclusive;

use super::{Analyzer, Config, Gap, ID, Kind, Mode, Shift};
use crate::directives::Directives;
use crate::format::layout::{VirtualLine, lines as virtual_lines};
use crate::rules::hyphens;
use crate::rules::support::event_compare::documents_with_analysis;
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
    consistent: Option<bool>,
}

impl<'a> Layout<'a> {
    pub(crate) fn collapse_scalars(
        &mut self,
        edits: &[crate::format::layout::PlanEdit],
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
                    bytes: text.len(),
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
            self.input.text(),
            &self.source_starts,
            &cfg.with_consistent_spaces(),
            Mode::Check,
        );
        analyzer.run(super::normalized_tokens(self.input, &self.source_starts));
        analyzer.diagnostics.is_empty()
    }
    pub(crate) fn new(input: Analysis<'a>, cfg: &Config) -> Self {
        Self::with_shapes(input, cfg, false)
    }

    pub(crate) fn format(input: Analysis<'a>, cfg: &Config) -> Self {
        Self::with_shapes(input, cfg, true)
    }

    fn with_shapes(input: Analysis<'a>, cfg: &Config, streaming: bool) -> Self {
        let mut starts = input.parsed().document_starts.clone();
        let directives = Directives::parse_with_analysis(input);
        let source_starts = if streaming {
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
                reshape_events(input, &mut lines, own_line)
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
            consistent: None,
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
            scalar_owners: _,
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
                        &gaps[gaps.partition_point(|gap| gap.line < line)
                            ..gaps.partition_point(|gap| gap.line <= line)],
                    );
                }
            }
            text
        };
        let mut output = TextVersion::new(render(&refused));
        output.inherit_event_capacity(input);
        let rollback = self.verify_events(&mut output, &mut refused);
        if rollback {
            let plan = output.analysis().plan(render(&refused));
            output
                .apply(plan)
                .expect("refusal plan belongs to the current epoch");
        }
        if cfg.require_consistent {
            let verified = if output.analysis().text() == buffer {
                input
            } else {
                output.analysis()
            };
            if refuse_inconsistent(verified, cfg, &mut refused) {
                let plan = output.analysis().plan(render(&refused));
                output
                    .apply(plan)
                    .expect("consistency plan belongs to the current epoch");
            }
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
        let input = self.input;
        let lines = &self.lines;
        let original_line_count = input.line_count();
        let line_starts = self.line_starts();
        let mut analyzer = Analyzer::new(input.text(), &line_starts, cfg, Mode::Target);
        let projection =
            crate::format::layout::projection::Projection::new(input, edits);
        analyzer.scalar_owners = folding.then(Vec::new);
        analyzer.lines = Some(lines);
        analyzer.run(super::normalized_tokens(input, &self.source_starts).map(
            |mut token| {
                let line = self.project_line(token.line, token.start);
                let end_line = if token.end_line >= original_line_count {
                    lines.len()
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
        ));
        let mut deltas = settle(lines, &analyzer.shifts);
        self.carry_scalar_blank_lines(&mut deltas);
        let refused = self.refusals(&analyzer.gaps, &analyzer.diagnostics);
        Targets {
            deltas,
            gaps: analyzer.gaps,
            refused,
            scalar_owners: analyzer.scalar_owners.unwrap_or_default(),
        }
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
        let mut analyzer = Analyzer::new(input.text(), &starts, cfg, Mode::Target);
        let consistent_cfg = cfg.with_consistent_spaces();
        let mut consistency =
            (edits.is_empty() && cfg.requires_consistency()).then(|| {
                Analyzer::new(
                    input.text(),
                    &self.source_starts,
                    &consistent_cfg,
                    Mode::Check,
                )
            });
        analyzer.lines = Some(&self.lines);
        analyzer.scalar_owners = folding.then(Vec::new);
        {
            let tokens = self.projected_tokens(edits, passes, &facts, &mut spacing);
            super::walk(tokens, |(raw, token), previous, next, after| {
                analyzer.advance(
                    token,
                    previous.map(|pair| &pair.1),
                    next.map(|pair| &pair.1),
                    after.map(|pair| &pair.1),
                );
                if let Some(check) = &mut consistency {
                    check.advance(
                        raw,
                        previous.map(|pair| &pair.0),
                        next.map(|pair| &pair.0),
                        after.map(|pair| &pair.0),
                    );
                }
            });
        }
        let Analyzer {
            shifts,
            gaps,
            diagnostics,
            scalar_owners,
            ..
        } = analyzer;
        let consistent = consistency.map(|check| check.diagnostics.is_empty());
        facts.into_inner().finish().map(|facts| {
            input.seed_layout_facts(facts);
            self.consistent = consistent;
            (
                StreamTargets {
                    shifts,
                    gaps,
                    diagnostics,
                    scalar_owners: scalar_owners.unwrap_or_default(),
                },
                spacing,
            )
        })
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
        edits: &'b [crate::format::layout::PlanEdit],
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
                while tokens.peek().is_some_and(|token| token.line == original) {
                    line.push_back(tokens.next().expect("peeked token exists"));
                }
                let next = facts.borrow_mut().sites.edits_through(
                    original + 1,
                    passes.colons.is_some(),
                    passes.hyphens.is_some(),
                );
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

    fn verify_events(
        &self,
        output: &mut TextVersion<'static>,
        refused: &mut [Option<Cause>],
    ) -> bool {
        if output.analysis().text() == self.input.text() {
            return false;
        }
        let strict = output.analysis().text().len() >= 1_000_000;
        let single = refused.len() == 1;
        if strict
            && single
            && let Ok(matches) = self.input.verify_strict(output.analysis())
        {
            let rollback = refused[0].is_none() && !matches;
            refused[0] = refused[0].or((!matches).then_some(Cause::Changed));
            if matches {
                output.mark_strict_valid();
            }
            return rollback;
        }
        let mut rollback = false;
        let mut compare = || {
            if single {
                let changed = !self.input.significant_events_match(output.analysis());
                rollback = refused[0].is_none() && changed;
                refused[0] = refused[0].or(changed.then_some(Cause::Changed));
            } else {
                let (original, _) = documents_with_analysis(self.input);
                let (rewritten, _) = documents_with_analysis(output.analysis());
                for (index, cause) in refused.iter_mut().enumerate() {
                    let changed = rewritten.get(index).map(|document| &document.events)
                        != Some(&original[index].events);
                    rollback |= cause.is_none() && changed;
                    *cause = cause.or(changed.then_some(Cause::Changed));
                }
            }
        };
        let valid = if strict && !single {
            std::thread::scope(|scope| {
                let text = output.analysis().text();
                let check = scope
                    .spawn(move || crate::lint::strict_parse_error(text).is_none());
                compare();
                check.join().expect("strict parser thread finishes")
            })
        } else {
            compare();
            false
        };
        if valid && !rollback {
            output.mark_strict_valid();
        }
        rollback
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

    fn carry_scalar_blank_lines(&self, deltas: &mut [isize]) {
        let input = self.input;
        let headers = input.headers();
        // These spaces are scalar content, so they move with the header.
        for line in input.content_blank_lines().iter().copied() {
            let header = headers
                .iter()
                .rev()
                .find(|header| header.line < line && line <= header.span.end.line())
                .expect("content line belongs to a block scalar");
            let header_line = header.line - 1;
            let header_index = self
                .lines
                .partition_point(|source| source.origin <= header_line)
                .saturating_sub(1);
            let header_text = input.line_content(header_line);
            let original_indent =
                header_text.len() - header_text.trim_start_matches(' ').len();
            let shaped_indent = self.lines[header_index].prefix;
            let shift = deltas[header_index] + super::to_isize(shaped_indent)
                - super::to_isize(original_indent);
            let first = self
                .lines
                .partition_point(|source| source.origin < line - 1);
            let last = self.lines.partition_point(|source| source.origin < line);
            for delta in &mut deltas[first..last] {
                *delta = shift;
            }
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
        let column = projection.column(original, byte, column);
        extra.map_or(column, |extra| extra.column(original, byte, column))
    }

    /// Each document's first [`Cause`] other than [`Cause::Changed`].
    fn refusals(
        &self,
        gaps: &[Gap],
        diagnostics: &[super::Violation],
    ) -> Vec<Option<Cause>> {
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
            if directives.is_disabled(gap.rule, self.lines[gap.line].origin + 1) {
                refuse(self.lines[gap.line].origin, Cause::Disabled);
            }
        }
        for (line, content, _) in self.input.lines() {
            if tab_in_indentation(content) {
                refuse(line, Cause::Tab);
            }
        }
        for problem in diagnostics {
            refuse(self.lines[problem.line - 1].origin, Cause::Unfollowable);
        }
        refused
    }
}

pub(crate) struct Targets {
    pub(crate) deltas: Vec<isize>,
    pub(crate) gaps: Vec<Gap>,
    pub(crate) refused: Vec<Option<Cause>>,
    pub(crate) scalar_owners: Vec<(usize, usize)>,
}

pub(crate) struct StreamTargets {
    shifts: Vec<Option<Shift>>,
    gaps: Vec<Gap>,
    diagnostics: Vec<super::Violation>,
    scalar_owners: Vec<(usize, usize)>,
}

impl StreamTargets {
    pub(crate) fn finish(self, layout: &Layout<'_>) -> Targets {
        let mut deltas = settle(&layout.lines, &self.shifts);
        layout.carry_scalar_blank_lines(&mut deltas);
        Targets {
            deltas,
            refused: layout.refusals(&self.gaps, &self.diagnostics),
            gaps: self.gaps,
            scalar_owners: self.scalar_owners,
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
) -> bool {
    let mut starts = input.parsed().document_starts.clone();
    starts[0] = 0;
    let mut rollback = false;
    for violation in super::check_with_analysis(input, &cfg.with_consistent_spaces()) {
        let document = starts.partition_point(|&start| start < violation.line) - 1;
        rollback |= refused[document].is_none();
        refused[document].get_or_insert(Cause::Inconsistent);
    }
    rollback
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
    let mut kept = Vec::new();
    shapes.retain(|&(line, split, dash)| {
        let keep = split.is_some() && comments.contains(&line);
        if keep {
            kept.push((line + 1, dash + 1));
        }
        !keep
    });
    if !shapes.is_empty() {
        apply_shapes(input.text(), lines, shapes);
    }
    kept
}

fn event_shape(
    input: Analysis<'_>,
    span: ryl_yaml_parser::Span,
    own_line: bool,
) -> Option<(usize, Option<usize>, usize)> {
    let line = span.start.line() - 1;
    let at = crate::rules::support::span_utils::marker_byte_offset(span.start).get();
    let prefix = &input.text()[input.byte_line_starts()[line]..at];
    let inline = prefix.trim_end_matches(' ').strip_suffix('-').map(str::len);
    let entry = inline.map(|column| (line, column)).or_else(|| {
        line.checked_sub(1).and_then(|above| {
            input
                .line_content(above)
                .trim_end_matches(' ')
                .strip_suffix('-')
                .filter(|prefix| !prefix.contains('#'))
                .map(|prefix| (above, prefix.len()))
        })
    });
    let (entry, dash) = entry?;
    if own_line && entry == line {
        Some((entry, Some(span.start.col()), dash))
    } else if !own_line && entry + 1 == line && span.start.col() > dash + 1 {
        Some((entry, None, dash))
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
