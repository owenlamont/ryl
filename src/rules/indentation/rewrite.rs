use std::ops::RangeInclusive;

use super::{Analyzer, Config, ID, Mode, Shift, locate, scan};
use crate::directives::Directives;
use crate::rules::support::event_compare::documents;
use crate::rules::support::line_syntax::split_lines_preserve_endings;
use crate::rules::support::punctuation::build_line_starts;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reindented {
    pub text: String,
    /// Each document left as it was.
    pub refused: Vec<Refusal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The document's 1-based lines.
    pub lines: RangeInclusive<usize>,
    /// An inline directive turned the rule off in it, rather than re-indenting it not being
    /// shown to keep its content.
    pub disabled: bool,
}

/// `buffer` with each line moved to where [`super::check`] under `cfg` expects it, one
/// document at a time. A document is left byte-identical where the analyzer cannot
/// follow it, a tab takes part in its indentation, an inline directive disables this
/// rule in it, or its events would change.
#[must_use]
pub fn reindent(buffer: &str, cfg: &Config) -> Reindented {
    let (original, parsed) = documents(buffer);
    if !parsed || original.is_empty() {
        return Reindented {
            text: buffer.to_string(),
            refused: Vec::new(),
        };
    }
    let chars: Vec<(usize, char)> = buffer.char_indices().collect();
    let line_starts = build_line_starts(&chars);
    let tokens = scan(buffer, &chars, &line_starts);
    let mut analyzer = Analyzer::new(&chars, &line_starts, cfg, Mode::Target);
    analyzer.run(&tokens);
    let lines: Vec<(&str, &str)> = split_lines_preserve_endings(buffer)
        .map(|(_, content, ending)| (content, ending))
        .collect();
    let deltas = settle(&lines, &analyzer.shifts);

    let mut starts: Vec<usize> = original
        .iter()
        .map(|document| locate(&line_starts, document.start).0)
        .collect();
    starts[0] = 0;
    let document_of = |line: usize| starts.partition_point(|&start| start <= line) - 1;
    let directives = Directives::parse(buffer);
    let mut refused = vec![false; starts.len()];
    for (line, (content, _)) in lines.iter().enumerate() {
        if directives.is_disabled(ID, line + 1) || tab_in_indentation(content) {
            refused[document_of(line)] = true;
        }
    }
    for problem in &analyzer.diagnostics {
        refused[document_of(problem.line - 1)] = true;
    }

    let render = |refused: &[bool]| {
        let mut text = String::with_capacity(buffer.len());
        for (line, (&(content, ending), &delta)) in
            lines.iter().zip(&deltas).enumerate()
        {
            let indent = content.len() - content.trim_start_matches(' ').len();
            if refused[document_of(line)] || delta == 0 || content.is_empty() {
                text.push_str(content);
            } else {
                text.push_str(&" ".repeat(indent.saturating_add_signed(delta)));
                text.push_str(&content[indent..]);
            }
            text.push_str(ending);
        }
        text
    };
    // Documents parse independently, so one round of refusals settles every document.
    let attempt = render(&refused);
    let (rewritten, _) = documents(&attempt);
    for (index, refuse) in refused.iter_mut().enumerate() {
        *refuse |= rewritten.get(index).map(|document| &document.events)
            != Some(&original[index].events);
    }
    Reindented {
        text: render(&refused),
        refused: (0..starts.len())
            .filter(|&index| refused[index])
            .map(|index| {
                let end = starts.get(index + 1).copied().unwrap_or(lines.len());
                Refusal {
                    lines: starts[index] + 1..=end,
                    disabled: (starts[index]..end)
                        .any(|line| directives.is_disabled(ID, line + 1)),
                }
            })
            .collect(),
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
