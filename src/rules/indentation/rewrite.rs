use std::collections::HashSet;
use std::ops::RangeInclusive;

use granit_parser::{Scanner, StrInput, Token, TokenType};

use super::{Analyzer, Config, Gap, ID, Kind, Mode, Shift, locate, scan};
use crate::directives::Directives;
use crate::rules::hyphens;
use crate::rules::support::event_compare::{Document, documents};
use crate::rules::support::line_syntax::{
    buffer_newline, split_lines_preserve_endings,
};
use crate::rules::support::punctuation::build_line_starts;

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
    /// The re-indented document parses to different events.
    Changed,
}

/// `buffer` with each line moved to where [`super::check`] under `cfg` expects it, one
/// document at a time, the spaces after a `-`, `?` or `:` that opens content closed to
/// one, and dash-line mappings joined or broken as `cfg` asks. A document is left
/// byte-identical for each [`Cause`].
#[must_use]
pub fn reindent(buffer: &str, cfg: &Config) -> Reindented {
    let (original, parsed) = documents(buffer);
    if !parsed || original.is_empty() {
        return Reindented {
            text: buffer.to_string(),
            refused: Vec::new(),
            kept_dash_lines: Vec::new(),
        };
    }
    let directives = Directives::parse(buffer);
    let lines: Vec<(&str, &str)> = split_lines_preserve_endings(buffer)
        .map(|(_, content, ending)| (content, ending))
        .collect();
    let starts = document_starts(buffer, &original);
    let document_of = |line: usize| starts.partition_point(|&start| start <= line) - 1;

    let (shaped, origin, kept_dash_lines) =
        reshape(buffer, &lines, cfg.dash_on_own_line, &directives);
    let chars: Vec<(usize, char)> = shaped.char_indices().collect();
    let line_starts = build_line_starts(&chars);
    let tokens = scan(&shaped, &chars, &line_starts);
    let mut analyzer = Analyzer::new(&chars, &line_starts, cfg, Mode::Target);
    analyzer.run(&tokens);
    let shaped_lines: Vec<(&str, &str)> = split_lines_preserve_endings(&shaped)
        .map(|(_, content, ending)| (content, ending))
        .collect();
    let deltas = settle(&shaped_lines, &analyzer.shifts);

    let mut refused: Vec<Option<Cause>> = vec![None; starts.len()];
    let mut refuse = |line: usize, cause: Cause| {
        refused[document_of(line)].get_or_insert(cause);
    };
    for line in 0..lines.len() {
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
    for (line, (content, _)) in lines.iter().enumerate() {
        if tab_in_indentation(content) {
            refuse(line, Cause::Tab);
        }
    }
    for problem in &analyzer.diagnostics {
        refuse(origin[problem.line - 1], Cause::Unfollowable);
    }

    let end_of = |index: usize| starts.get(index + 1).copied().unwrap_or(lines.len());
    let render = |refused: &[Option<Cause>]| {
        let mut text = String::with_capacity(buffer.len());
        for (index, cause) in refused.iter().enumerate() {
            if cause.is_some() {
                for &(content, ending) in &lines[starts[index]..end_of(index)] {
                    text.push_str(content);
                    text.push_str(ending);
                }
                continue;
            }
            let first = origin.partition_point(|&line| line < starts[index]);
            let last = origin.partition_point(|&line| line < end_of(index));
            for line in first..last {
                render_line(
                    &mut text,
                    shaped_lines[line],
                    deltas[line],
                    &analyzer.gaps[line],
                );
            }
        }
        text
    };
    // Documents parse independently, so one round of refusals settles every document.
    let attempt = render(&refused);
    let (rewritten, _) = documents(&attempt);
    for (index, cause) in refused.iter_mut().enumerate() {
        let changed = rewritten.get(index).map(|document| &document.events)
            != Some(&original[index].events);
        *cause = cause.or(changed.then_some(Cause::Changed));
    }
    Reindented {
        text: render(&refused),
        kept_dash_lines,
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

/// For each of `cfgs`, how many lines led by a token [`reindent`] would move: a line
/// inside a multi-line scalar, a block scalar's body included, never counts.
#[must_use]
pub fn moved_lines(buffer: &str, cfgs: &[Config]) -> Vec<usize> {
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let line_starts = build_line_starts(&chars);
    let tokens = scan(buffer, &chars, &line_starts);
    cfgs.iter()
        .map(|cfg| {
            let mut analyzer = Analyzer::new(&chars, &line_starts, cfg, Mode::Target);
            analyzer.run(&tokens);
            analyzer
                .shifts
                .iter()
                .filter(|shift| matches!(shift, Some(Shift::Token { delta, .. }) if *delta != 0))
                .count()
        })
        .collect()
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
    delta: isize,
    gaps: &[Gap],
) {
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

/// `buffer` with each block mapping in a block sequence joined onto its `-` line, or
/// broken onto the next, as `dash_on_own_line` asks, keeping every token's column; and the
/// line of `lines` each output line starts on. A dash line carrying a node property or a
/// comment, or one where `hyphens` is disabled, is left as it is.
fn reshape(
    buffer: &str,
    lines: &[(&str, &str)],
    dash_on_own_line: Option<bool>,
    directives: &Directives,
) -> (String, Vec<usize>, Vec<(usize, usize)>) {
    let mut joins = vec![None; lines.len()];
    let mut breaks = vec![None; lines.len()];
    let mut kept = Vec::new();
    if let Some(own_line) = dash_on_own_line {
        let chars: Vec<(usize, char)> = buffer.char_indices().collect();
        let line_starts = build_line_starts(&chars);
        let tokens = scan(buffer, &chars, &line_starts);
        let commented: HashSet<usize> = Scanner::new(StrInput::new(buffer))
            .map_while(Result::ok)
            .map(Token::into_parts)
            .filter(|(_, token)| matches!(token, TokenType::Comment(_)))
            .map(|(span, _)| locate(&line_starts, span.start.index()).0)
            .collect();
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
                && lines[entry.line].0.trim_end().len() == entry.column + 1
            {
                joins[entry.line] = Some(entry.column);
            }
        }
    }
    let mut text = String::with_capacity(buffer.len());
    let mut origin = Vec::with_capacity(lines.len());
    let mut line = 0;
    while line < lines.len() {
        let (content, ending) = lines[line];
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
    (text, origin, kept)
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
    let mut deltas = Vec::with_capacity(lines.len());
    for (line, (content, _)) in lines.iter().enumerate() {
        let column = super::to_isize(content.len() - content.trim_start().len());
        let delta = match shifts[line] {
            Some(shift) => shift.delta(),
            None if content.trim_start().starts_with('#') => {
                let next = (line + 1..lines.len()).find_map(token);
                let previous = (0..line).rev().find_map(token);
                match (next, previous) {
                    (Some((found, delta)), _) if found == column => delta,
                    (_, Some((found, delta))) if found == column => delta,
                    (Some((found, delta)), _) => found + delta - column,
                    (None, previous) => previous.map_or(0, |(_, delta)| delta),
                }
            }
            None => deltas.last().copied().unwrap_or(0),
        };
        deltas.push(delta.max(-column));
    }
    deltas
}
