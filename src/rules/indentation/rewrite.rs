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
    source_starts: Vec<usize>,
    pub(crate) lines: Vec<VirtualLine<'a>>,
    pub(crate) shaped: bool,
    projection: Option<Vec<usize>>,
    pub(crate) kept_dash_lines: Vec<(usize, usize)>,
}

impl<'a> Layout<'a> {
    pub(crate) fn collapse_scalars(
        &mut self,
        edits: &[crate::text_version::edits::Edit],
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
            collapses.push((first, last, start.get(), end.get()));
            compatible &= !text.contains(['\n', '\r'])
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
            if let Some((_, last, start, end)) =
                collapses.next_if(|(first, _, _, _)| *first == index)
            {
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
                line.bridge = Some((start, end));
            }
            self.lines.push(line);
        }
        self.reindex();
        true
    }

    pub(crate) fn consistent(&self, cfg: &Config) -> bool {
        if let Some(facts) = self
            .input
            .layout_facts()
            .filter(|facts| facts.layout.config == *cfg)
        {
            return facts.layout.consistent;
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
        let dry = input
            .layout_facts()
            .filter(|facts| facts.layout.config == *cfg);
        let mut starts = input.parsed().document_starts.clone();
        let directives = Directives::parse_with_analysis(input);
        let source_starts = if dry.is_some() {
            input.byte_line_starts().to_vec()
        } else {
            super::normalized_line_starts(input)
        };
        let mut lines = virtual_lines(input, &source_starts);
        let parsed = input.parsed().valid && !starts.is_empty();
        if parsed {
            starts[0] = 0;
        } else {
            starts.clear();
        }
        let kept_dash_lines = if let Some(facts) = dry {
            facts.layout.kept_dash_lines.clone()
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
        let shaped = dry.is_some_and(|facts| facts.layout.shaped)
            || lines.len() != input.lines().count()
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
        };
        layout.reindex();
        layout
    }

    fn reindent_snapshot(&self, cfg: &Config) -> (Reindented, TextVersion<'static>) {
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
        let lines = &self.lines;
        let original_line_count = input.line_contents().len();
        let starts = &self.starts;
        let Targets {
            deltas,
            gaps,
            mut refused,
            scalar_owners: _,
        } = self.targets(cfg, &[], false);
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
                        .lines()
                        .skip(starts[index])
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
        &self,
        cfg: &Config,
        edits: &[crate::text_version::edits::Edit],
        folding: bool,
    ) -> Targets {
        if let Some(facts) = self
            .input
            .layout_facts()
            .filter(|facts| facts.layout.config == *cfg)
        {
            let mut deltas = facts.layout.deltas.clone();
            self.carry_scalar_blank_lines(&mut deltas);
            return Targets {
                deltas,
                gaps: facts.layout.gaps.clone(),
                refused: self.refusals(&facts.layout.gaps, &facts.layout.diagnostics),
                scalar_owners: facts.layout.scalar_owners.clone(),
            };
        }
        let input = self.input;
        let lines = &self.lines;
        let original_line_count = input.line_contents().len();
        let mut line_starts: Vec<_> = lines
            .iter()
            .map(|line| self.source_starts[line.origin])
            .collect();
        line_starts.push(*self.source_starts.last().expect("scanner has an EOF line"));
        let mut analyzer = Analyzer::new(input.text(), &line_starts, cfg, Mode::Target);
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
                        edits,
                        line,
                        token.line,
                        token.start,
                        token.column,
                    );
                    token.end_column = self.project_column(
                        edits,
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
        let lines = input.line_contents();
        let headers = input.headers();
        // These spaces are scalar content, so they move with the header.
        for line in
            crate::rules::trailing_spaces::content_blank_lines_with_analysis(input)
        {
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
            let original_indent = lines[header_line].len()
                - lines[header_line].trim_start_matches(' ').len();
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
        edits: &[crate::text_version::edits::Edit],
        index: usize,
        original: usize,
        byte: usize,
        column: usize,
    ) -> usize {
        if let Some((start, end)) = self.lines.get(index).and_then(|line| line.bridge)
            && byte >= end
        {
            let source_line = self.lines[index].origin;
            let from = self.source_starts[source_line];
            let prefix = self.input.text()[from..start].chars().count();
            let prefix = crate::format::layout::column(
                self.input,
                edits,
                source_line,
                start,
                prefix,
            );
            let replacement =
                &edits[edits.partition_point(|(from, _, _)| from.get() < start)].2;
            return prefix
                + replacement.chars().count()
                + self.input.text()[end..byte].chars().count();
        }
        crate::format::layout::column(self.input, edits, original, byte, column)
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
        for line in 0..self.input.line_contents().len() {
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

pub(crate) struct DryLayout {
    config: Config,
    deltas: Vec<isize>,
    gaps: Vec<Gap>,
    diagnostics: Vec<super::Violation>,
    pub(crate) consistent: bool,
    shaped: bool,
    kept_dash_lines: Vec<(usize, usize)>,
    scalar_owners: Vec<(usize, usize)>,
}

pub(crate) fn stream_layout(
    input: Analysis<'_>,
    cfg: &Config,
    tokens: impl Iterator<Item = (granit_parser::Span, crate::text_version::lexical::Kind)>,
    folding: bool,
) -> DryLayout {
    use crate::text_version::lexical::Kind as LexicalKind;
    let starts = input.byte_line_starts();
    let lines = virtual_lines(input, starts);
    let mut commented = std::collections::HashSet::new();
    let mut candidates = Vec::new();
    let mut analyzer = Analyzer::new(input.text(), starts, cfg, Mode::Target);
    analyzer.scalar_owners = folding.then(Vec::new);
    analyzer.lines = Some(&lines);
    let consistent_cfg = cfg.with_consistent_spaces();
    let mut consistency = cfg
        .requires_consistency()
        .then(|| Analyzer::new(input.text(), starts, &consistent_cfg, Mode::Check));
    let tokens = tokens.inspect(|(span, kind)| {
        if matches!(kind, LexicalKind::Comment { .. }) {
            commented.insert(span.start.line() - 1);
        }
    });
    analyzer.run_with(
        super::normalized_tokens_from(input.text(), starts, tokens),
        |token, previous, next, after| {
            if let Some(check) = &mut consistency {
                check.advance(token, previous, next, after);
            }
            if let Some(own_line) = cfg.dash_on_own_line
                && let Some(start) = next
                && let Some(shape) = shape(token, start, own_line, &lines)
            {
                candidates.push(shape);
            }
        },
    );
    let mut kept_dash_lines = Vec::new();
    let mut shaped = false;
    for (line, split, dash) in candidates {
        let keep_dash = split.is_some() && commented.contains(&line);
        kept_dash_lines.extend(keep_dash.then_some((line + 1, dash + 1)));
        shaped |= !keep_dash;
    }
    DryLayout {
        config: *cfg,
        deltas: settle(&lines, &analyzer.shifts),
        gaps: analyzer.gaps,
        diagnostics: analyzer.diagnostics,
        consistent: consistency.is_none_or(|check| check.diagnostics.is_empty()),
        shaped,
        kept_dash_lines,
        scalar_owners: analyzer.scalar_owners.unwrap_or_default(),
    }
}

fn shape(
    entry: &super::Token,
    start: &super::Token,
    own_line: bool,
    lines: &[VirtualLine<'_>],
) -> Option<(usize, Option<usize>, usize)> {
    if entry.kind != Kind::BlockEntry || start.kind != Kind::BlockMappingStart {
        return None;
    }
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
            .iter()
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
    kept
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
