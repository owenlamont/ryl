use std::cell::OnceCell;
use std::collections::HashSet;
use std::ops::RangeInclusive;

use granit_parser::{Event, Span};

use super::{
    Analyzer, Config, Gap, ID, Kind, Mode, Shift, locate, scan, scan_with_comments,
};
use crate::directives::Directives;
use crate::rules::hyphens;
use crate::rules::support::event_compare::{
    Document, documents, documents_from_events,
};
use crate::rules::support::line_syntax::{
    buffer_newline, split_lines_preserve_endings,
};
use crate::rules::support::punctuation::build_line_starts;
use crate::rules::support::span_utils::CharPos;

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
    reindent_with_events(buffer, cfg, None)
}

pub(crate) fn reindent_with_events(
    buffer: &str,
    cfg: &Config,
    events: Option<&[(Event<'_>, Span)]>,
) -> Reindented {
    Prepared::new(buffer, cfg.dash_on_own_line, events).reindent(cfg)
}

/// What [`reindent`] derives from `buffer` alone, so several widths can share it.
pub(crate) struct Prepared<'a> {
    buffer: &'a str,
    original: Vec<Document<'a>>,
    pub(crate) parsed: bool,
    directives: Directives,
    lines: Vec<(&'a str, &'a str)>,
    starts: Vec<usize>,
    shaped: String,
    origin: Vec<usize>,
    kept_dash_lines: Vec<(usize, usize)>,
    chars: Vec<(usize, char)>,
    line_starts: Vec<CharPos>,
    tokens: Vec<super::Token>,
    scalar_blank_headers: OnceCell<Vec<(usize, usize)>>,
}

impl<'a> Prepared<'a> {
    pub(crate) fn for_config(
        buffer: &'a str,
        cfg: &Config,
        events: Option<&[(Event<'a>, Span)]>,
    ) -> Self {
        Self::new(buffer, cfg.dash_on_own_line, events)
    }

    fn new(
        buffer: &'a str,
        dash_on_own_line: Option<bool>,
        events: Option<&[(Event<'a>, Span)]>,
    ) -> Self {
        let (original, parsed) = events.map_or_else(
            || documents(buffer),
            |events| (documents_from_events(events), true),
        );
        let directives = Directives::parse(buffer);
        let lines: Vec<(&str, &str)> = split_lines_preserve_endings(buffer)
            .map(|(_, content, ending)| (content, ending))
            .collect();
        let parsed = parsed && !original.is_empty();
        let starts = if parsed {
            document_starts(buffer, &original)
        } else {
            Vec::new()
        };
        let (shaped, origin, kept_dash_lines, scanned) = if parsed {
            reshape(buffer, &lines, dash_on_own_line, &directives)
        } else {
            (String::new(), Vec::new(), Vec::new(), None)
        };
        let chars: Vec<(usize, char)> = shaped.char_indices().collect();
        let (tokens, line_starts) =
            scanned.filter(|_| shaped == buffer).unwrap_or_else(|| {
                let mut starts = build_line_starts(&chars);
                let tokens = scan(&shaped, &chars, &mut starts);
                (tokens, starts)
            });
        Self {
            buffer,
            original,
            parsed,
            directives,
            lines,
            starts,
            shaped,
            origin,
            kept_dash_lines,
            chars,
            line_starts,
            tokens,
            scalar_blank_headers: OnceCell::new(),
        }
    }

    pub(crate) fn structural_width(&self) -> Option<u8> {
        let mut parents: Vec<&super::Token> = Vec::new();
        let mut width = None;
        let mut nested = false;
        let mut observe = |step: usize| {
            let step = u8::try_from(step).ok()?;
            if !(2..=8).contains(&step)
                || width.replace(step).is_some_and(|previous| previous != step)
            {
                return None;
            }
            Some(())
        };
        for token in &self.tokens {
            match token.kind {
                Kind::BlockMappingStart | Kind::BlockSequenceStart => {
                    if let Some(parent) = parents.last()
                        && token.line != parent.line
                        && token.column > parent.column
                    {
                        observe(token.column - parent.column)?;
                        nested = true;
                    } else if parents.is_empty() && token.column != 0 {
                        return None;
                    }
                    parents.push(token);
                }
                Kind::BlockEnd => {
                    parents.pop();
                }
                Kind::Scalar {
                    style:
                        granit_parser::ScalarStyle::Literal
                        | granit_parser::ScalarStyle::Folded,
                    ..
                } => {
                    if self.chars[token.start + 1..]
                        .iter()
                        .map(|&(_, ch)| ch)
                        .take_while(|&ch| {
                            ch.is_ascii_digit() || matches!(ch, '+' | '-')
                        })
                        .any(|ch| ch.is_ascii_digit())
                    {
                        return None;
                    }
                    let indent =
                        (token.line + 1..=super::last_line(token)).find_map(|line| {
                            let start = self.line_starts[line].get();
                            let indent = super::count_spaces(&self.chars, start);
                            (!super::char_at(&self.chars, start + indent)
                                .is_some_and(super::is_break))
                            .then_some(indent)
                        });
                    if let Some(indent) = indent {
                        observe(indent.saturating_sub(parents.last()?.column))?;
                    }
                }
                _ => {}
            }
        }
        width.filter(|_| nested)
    }

    pub(crate) fn reindent(&self, cfg: &Config) -> Reindented {
        if !self.parsed {
            return Reindented {
                text: self.buffer.to_string(),
                refused: Vec::new(),
                kept_dash_lines: Vec::new(),
            };
        }
        let (lines, starts, origin) = (&self.lines, &self.starts, &self.origin);
        let mut analyzer =
            Analyzer::new(&self.chars, &self.line_starts, cfg, Mode::Target);
        analyzer.run(&self.tokens);
        let shaped_lines: Vec<(&str, &str)> =
            split_lines_preserve_endings(&self.shaped)
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
                .collect();
        let mut deltas = settle(&shaped_lines, &analyzer.shifts);
        self.carry_scalar_blank_lines(&shaped_lines, &mut deltas);
        let mut refused = self.refusals(&analyzer);
        let end_of =
            |index: usize| starts.get(index + 1).copied().unwrap_or(lines.len());
        let render = |refused: &[Option<Cause>]| {
            let mut text = String::with_capacity(self.buffer.len());
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
        let attempt = render(&refused);
        let mut reverted = false;
        if attempt != self.buffer {
            let (rewritten, _) = documents(&attempt);
            for (index, cause) in refused.iter_mut().enumerate() {
                let changed = rewritten.get(index).map(|document| &document.events)
                    != Some(&self.original[index].events);
                reverted |= cause.is_none() && changed;
                *cause = cause.or(changed.then_some(Cause::Changed));
            }
        }
        let mut text = if reverted { render(&refused) } else { attempt };
        if cfg.require_consistent {
            let (output_documents, _) = documents(&text);
            let output_starts = document_starts(&text, &output_documents);
            let mut inconsistent = false;
            for violation in super::check(&text, &cfg.with_consistent_spaces()) {
                let document =
                    output_starts.partition_point(|&start| start < violation.line) - 1;
                inconsistent |= refused[document].is_none();
                refused[document].get_or_insert(Cause::Inconsistent);
            }
            if inconsistent {
                text = render(&refused);
            }
        }
        Reindented {
            text,
            kept_dash_lines: self.kept_dash_lines.clone(),
            refused: (0..starts.len())
                .filter_map(|index| {
                    refused[index].map(|cause| Refusal {
                        lines: starts[index] + 1..=end_of(index),
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
        let blank_headers = self.scalar_blank_headers.get_or_init(|| {
            if !self.lines.iter().any(|(line, _)| {
                !line.is_empty() && line.bytes().all(|byte| byte == b' ')
            }) {
                return Vec::new();
            }
            let headers = crate::rules::block_scalar_chomping::headers(self.buffer);
            crate::rules::trailing_spaces::content_blank_lines(self.buffer)
                .into_iter()
                .map(|line| {
                    let header = headers
                        .iter()
                        .rev()
                        .find(|header| {
                            header.line < line && line <= header.span.end.line()
                        })
                        .expect("content line belongs to a block scalar");
                    (line, header.line - 1)
                })
                .collect()
        });
        // These spaces are scalar content, so they move with the header.
        for &(line, header_line) in blank_headers {
            let header_index = self
                .origin
                .partition_point(|&source| source <= header_line)
                .saturating_sub(1);
            let original_indent = self.lines[header_line].0.len()
                - self.lines[header_line].0.trim_start_matches(' ').len();
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
        let start = self.line_starts[line].get();
        start > 0 && self.chars[start - 1].1 == '\u{feff}'
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
        for (line, (content, _)) in self.lines.iter().enumerate() {
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

/// The 0-based line each of `documents` starts on, the first taking any lines before it.
fn document_starts(buffer: &str, documents: &[Document<'_>]) -> Vec<usize> {
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let line_starts = build_line_starts(&chars);
    let mut starts: Vec<usize> = documents
        .iter()
        .map(|document| locate(&line_starts, document.start).0)
        .collect();
    starts[0] = 0;
    starts
}

/// Pushes `line` moved by `delta`, with `gaps` closed.
fn render_line(
    text: &mut String,
    (content, ending): (&str, &str),
    bom: bool,
    delta: isize,
    gaps: &[Gap],
) {
    if bom {
        text.push('\u{feff}');
    }
    let indent = content.len() - content.trim_start_matches(' ').len();
    let placed = indent.saturating_add_signed(delta);
    text.push_str(&" ".repeat(placed * usize::from(!content.is_empty())));
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

type ReshapeResult = (
    String,
    Vec<usize>,
    Vec<(usize, usize)>,
    Option<(Vec<super::Token>, Vec<CharPos>)>,
);

/// `buffer` with each block mapping in a block sequence joined onto its `-` line, or
/// broken onto the next, as `dash_on_own_line` asks, keeping every token's column; and the
/// line of `lines` each output line starts on. A dash line carrying a node property or a
/// comment, or one where `hyphens` is disabled, is left as it is.
fn reshape(
    buffer: &str,
    lines: &[(&str, &str)],
    dash_on_own_line: Option<bool>,
    directives: &Directives,
) -> ReshapeResult {
    let mut joins = vec![None; lines.len()];
    let mut breaks = vec![None; lines.len()];
    let mut kept = Vec::new();
    let mut scanned = None;
    if let Some(own_line) = dash_on_own_line {
        let chars: Vec<(usize, char)> = buffer.char_indices().collect();
        let mut line_starts = build_line_starts(&chars);
        let mut commented = HashSet::new();
        let tokens =
            scan_with_comments(buffer, &chars, &mut line_starts, Some(&mut commented));
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
        scanned = Some((tokens, line_starts));
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
            text.push_str(&" ".repeat(at));
            text.push_str(&content[at..]);
        } else {
            text.push_str(content);
        }
        text.push_str(ending);
        line += 1;
    }
    (text, origin, kept, scanned)
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
