use std::borrow::Cow;
use std::ops::RangeInclusive;

use super::{Analyzer, Config, Gap, ID, Kind, Mode, Shift};
use crate::directives::Directives;
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
    Prepared::new(input, cfg.dash_on_own_line).reindent_snapshot(cfg)
}

struct Prepared<'a> {
    input: Analysis<'a>,
    has_bom: bool,
    parsed: bool,
    directives: Directives,
    starts: Vec<usize>,
    lines: Vec<(&'a str, &'a str)>,
    shaped: Option<TextVersion<'static>>,
    origin: Vec<usize>,
    kept_dash_lines: Vec<(usize, usize)>,
}

impl<'a> Prepared<'a> {
    fn new(input: Analysis<'a>, dash_on_own_line: Option<bool>) -> Self {
        let mut starts = input.parsed().document_starts.clone();
        let directives = Directives::parse_with_analysis(input);
        let lines: Vec<_> = input
            .lines()
            .map(|(_, content, ending)| (content, ending))
            .collect();
        let parsed = input.parsed().valid && !starts.is_empty();
        if parsed {
            starts[0] = 0;
        } else {
            starts.clear();
        }
        let (shaped, origin, kept_dash_lines) = if parsed {
            reshape(input, &lines, dash_on_own_line, &directives)
        } else {
            (None, Vec::new(), Vec::new())
        };
        let shaped = shaped.map(|text| {
            let mut candidate = TextVersion::new(text);
            candidate.inherit_event_capacity(input);
            candidate
        });
        Self {
            input,
            has_bom: input.text().contains('\u{feff}'),
            parsed,
            directives,
            starts,
            lines,
            shaped,
            origin,
            kept_dash_lines,
        }
    }

    fn shaped(&self) -> Analysis<'_> {
        self.shaped
            .as_ref()
            .map_or(self.input, TextVersion::analysis)
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
        let (starts, origin) = (&self.starts, &self.origin);
        let shaped = self.shaped();
        let scanned = shaped.indentation();
        let mut analyzer =
            Analyzer::new(shaped.text(), &scanned.line_starts, cfg, Mode::Target);
        analyzer.run(&scanned.tokens);
        let shaped_lines: Cow<'_, [(&str, &str)]> =
            if self.shaped.is_none() && !self.has_bom {
                Cow::Borrowed(&self.lines)
            } else {
                Cow::Owned(
                    shaped
                        .lines()
                        .map(|(line, content, ending)| {
                            let content = if self.bom_prefix(line) {
                                content
                                    .strip_prefix('\u{feff}')
                                    .expect("BOM prefix matches the scanned line")
                            } else {
                                content
                            };
                            (content, ending)
                        })
                        .collect(),
                )
            };
        let mut deltas = settle(&shaped_lines, &analyzer.shifts);
        self.carry_scalar_blank_lines(&shaped_lines, &mut deltas);
        let mut refused = self.refusals(&analyzer);
        let end_of =
            |index: usize| starts.get(index + 1).copied().unwrap_or(lines.len());
        let render = |refused: &[Option<Cause>]| {
            let mut text = String::with_capacity(buffer.len());
            for (index, cause) in refused.iter().enumerate() {
                if cause.is_some() {
                    text.extend(
                        lines[starts[index]..end_of(index)]
                            .iter()
                            .flat_map(|&(content, ending)| [content, ending]),
                    );
                    continue;
                }
                let first = origin.partition_point(|&line| line < starts[index]);
                let last = origin.partition_point(|&line| line < end_of(index));
                for line in first..last {
                    render_line(
                        &mut text,
                        shaped_lines[line],
                        self.bom_prefix(line),
                        deltas[line],
                        &analyzer.gaps[line],
                    );
                }
            }
            text
        };
        // Documents parse independently, so one round of refusals settles every document.
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
            self.result(output.to_string(), &refused, lines.len()),
            output,
        )
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

    fn carry_scalar_blank_lines(
        &self,
        shaped_lines: &[(&str, &str)],
        deltas: &mut [isize],
    ) {
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
                .origin
                .partition_point(|&source| source <= header_line)
                .saturating_sub(1);
            let original_indent = lines[header_line].len()
                - lines[header_line].trim_start_matches(' ').len();
            let shaped_indent = shaped_lines[header_index].0.len()
                - shaped_lines[header_index].0.trim_start_matches(' ').len();
            let shift = deltas[header_index] + super::to_isize(shaped_indent)
                - super::to_isize(original_indent);
            let first = self.origin.partition_point(|&source| source < line - 1);
            let last = self.origin.partition_point(|&source| source < line);
            for delta in &mut deltas[first..last] {
                *delta = shift;
            }
        }
    }

    fn bom_prefix(&self, line: usize) -> bool {
        if !self.has_bom {
            return false;
        }
        let start = self.shaped().indentation().line_starts[line];
        self.shaped().text()[..start].ends_with('\u{feff}')
    }

    /// Each document's first [`Cause`] other than [`Cause::Changed`].
    fn refusals(&self, analyzer: &Analyzer) -> Vec<Option<Cause>> {
        let (directives, origin) = (&self.directives, &self.origin);
        let mut refused: Vec<Option<Cause>> = vec![None; self.starts.len()];
        let mut refuse = |line: usize, cause: Cause| {
            let document = self.starts.partition_point(|&start| start <= line) - 1;
            refused[document].get_or_insert(cause);
        };
        for line in 0..self.lines.len() {
            if directives.is_disabled(ID, line + 1) {
                refuse(line, Cause::Disabled);
            }
        }
        for (line, gaps) in analyzer.gaps.iter().enumerate().take(origin.len()) {
            if gaps
                .iter()
                .any(|gap| directives.is_disabled(gap.rule, origin[line] + 1))
            {
                refuse(origin[line], Cause::Disabled);
            }
        }
        for (line, content, _) in self.input.lines() {
            if tab_in_indentation(content) {
                refuse(line, Cause::Tab);
            }
        }
        for problem in &analyzer.diagnostics {
            refuse(origin[problem.line - 1], Cause::Unfollowable);
        }
        refused
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

/// Pushes `line` moved by `delta`, with `gaps` closed.
fn render_line(
    text: &mut String,
    (content, ending): (&str, &str),
    bom: bool,
    delta: isize,
    gaps: &[Gap],
) {
    if !bom && delta == 0 && gaps.is_empty() {
        text.push_str(content);
        text.push_str(ending);
        return;
    }
    if bom {
        text.push('\u{feff}');
    }
    let indent = content.len() - content.trim_start_matches(' ').len();
    let placed = indent.saturating_add_signed(delta);
    text.extend(std::iter::repeat_n(
        ' ',
        placed * usize::from(!content.is_empty()),
    ));
    let mut rest = &content[indent..];
    let mut at = indent;
    for gap in gaps {
        let keep = gap.column + 2 - at;
        text.push_str(&rest[..keep]);
        rest = &rest[keep + gap.removed..];
        at = gap.column + 2 + gap.removed;
    }
    text.push_str(rest);
    text.push_str(ending);
}

type ReshapeResult = (Option<String>, Vec<usize>, Vec<(usize, usize)>);

/// `buffer` with each block mapping in a block sequence joined onto its `-` line, or
/// broken onto the next, as `dash_on_own_line` asks, keeping every token's column; and the
/// line of `lines` each output line starts on. A dash line carrying a node property or a
/// comment, or one where `hyphens` is disabled, is left as it is.
fn reshape(
    input: Analysis<'_>,
    lines: &[(&str, &str)],
    dash_on_own_line: Option<bool>,
    directives: &Directives,
) -> ReshapeResult {
    let buffer = input.text();
    let mut joins = vec![None; lines.len()];
    let mut breaks = vec![None; lines.len()];
    let mut kept = Vec::new();
    if let Some(own_line) = dash_on_own_line {
        let scanned = input.indentation();
        let tokens = &scanned.tokens;
        let commented = &scanned.comments;
        for pair in tokens.windows(2) {
            let (entry, start) = (pair[0], pair[1]);
            if entry.kind != Kind::BlockEntry
                || start.kind != Kind::BlockMappingStart
                || directives.is_disabled(hyphens::ID, entry.line + 1)
                || directives.is_disabled(hyphens::ID, start.line + 1)
            {
                continue;
            }
            if own_line && start.line == entry.line {
                if commented.contains(&entry.line) {
                    kept.push((entry.line + 1, entry.column + 1));
                } else {
                    breaks[entry.line] = Some((entry.column, start.column));
                }
            } else if !own_line
                && start.line == entry.line + 1
                && start.column > entry.column + 1
                && lines[entry.line]
                    .0
                    .trim_start_matches('\u{feff}')
                    .trim_end()
                    .len()
                    == entry.column + 1
            {
                joins[entry.line] = Some(entry.column);
            }
        }
    }
    if joins.iter().all(Option::is_none) && breaks.iter().all(Option::is_none) {
        return (None, (0..lines.len()).collect(), kept);
    }
    let mut text = String::with_capacity(buffer.len());
    let mut origin = Vec::with_capacity(lines.len());
    let mut line = 0;
    while line < lines.len() {
        let (content, ending) = lines[line];
        let content = if (joins[line].is_some() || breaks[line].is_some())
            && content.starts_with('\u{feff}')
        {
            text.push('\u{feff}');
            &content['\u{feff}'.len_utf8()..]
        } else {
            content
        };
        origin.push(line);
        if let Some(dash) = joins[line] {
            let (below, below_ending) = lines[line + 1];
            text.push_str(&content[..=dash]);
            text.push_str(&below[dash + 1..]);
            text.push_str(below_ending);
            line += 2;
            continue;
        }
        if let Some((dash, at)) = breaks[line] {
            text.push_str(&content[..=dash]);
            text.push_str(if ending.is_empty() {
                buffer_newline(buffer)
            } else {
                ending
            });
            origin.push(line);
            text.extend(std::iter::repeat_n(' ', at));
            text.push_str(&content[at..]);
        } else {
            text.push_str(content);
        }
        text.push_str(ending);
        line += 1;
    }
    (Some(text), origin, kept)
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
fn settle(lines: &[(&str, &str)], shifts: &[Option<Shift>]) -> Vec<isize> {
    let token = |line: usize| match shifts[line] {
        Some(Shift::Token { found, delta }) => Some((found, delta)),
        _ => None,
    };
    let mut following = vec![None; lines.len()];
    let mut nearest = None;
    for line in (0..lines.len()).rev() {
        following[line] = nearest;
        nearest = token(line).or(nearest);
    }
    let mut deltas = Vec::with_capacity(lines.len());
    let mut previous = None;
    for (line, (content, _)) in lines.iter().enumerate() {
        let column = super::to_isize(content.len() - content.trim_start().len());
        let delta = match shifts[line] {
            Some(shift) => shift.delta(),
            None if content.trim_start().starts_with('#') => {
                match (following[line], previous) {
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
