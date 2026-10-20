use std::ops::Range;

use crate::text_version::lexical::Kind;
use crate::text_version::{Analysis, TextVersion};
use granit_parser::ScalarStyle;

use super::{Config, ID};
use crate::directives::{Directives, directive_scope};
use crate::rules::support::line_syntax::{
    buffer_newline, split_lines_preserve_endings,
};
use crate::rules::support::span_utils::{
    BytePos, apply_replacements, marker_byte_offset,
};

#[derive(Debug, Clone, Copy)]
pub struct Fold {
    pub width: u16,
    pub indent: u8,
}

#[derive(Clone)]
pub(crate) struct FoldableScalar {
    pub(crate) content: Range<usize>,
    pub(crate) owner: Option<(usize, usize)>,
    pub(crate) style: ScalarStyle,
}

impl Fold {
    #[must_use]
    pub fn check_config(self) -> Config {
        Config {
            max: i64::from(self.width),
            allow_non_breakable_words: false,
            allow_non_breakable_inline_mappings: false,
        }
    }
}

/// `buffer` with each over-long line of a plain or quoted scalar in block context, or of a
/// `>` scalar at its content indent, split at single spaces; `None` when nothing folds.
/// A line `line-length` is disabled on stays whole, as does one ending in a directive
/// comment, which a fold would move.
#[must_use]
pub fn fold(buffer: &str, cfg: Fold) -> Option<String> {
    let source = TextVersion::new(buffer);
    let input = source.analysis();
    let directives = Directives::parse_with_analysis(input);
    let mut start = 0;
    let lines: Vec<(usize, &str)> = split_lines_preserve_endings(buffer)
        .map(|(_, content, ending)| {
            let line = (start, content);
            start += content.len() + ending.len();
            line
        })
        .collect();
    let line_of =
        |offset: usize| lines.partition_point(|(start, _)| *start <= offset) - 1;
    let newline = buffer_newline(buffer);
    let mut edits = Vec::new();
    for FoldableScalar {
        content: span,
        owner,
        style,
    } in foldable_scalars(input)
    {
        let owner = owner.map(|(_, column)| column);
        // granit rejects a quoted mapping value continued one column past its key.
        let min_step = if style == ScalarStyle::Plain { 1 } else { 2 };
        let (first, last) = (line_of(span.start), line_of(span.end - 1));
        let folded = style == ScalarStyle::Folded;
        let content = span.start - lines[first].0;
        // An empty `>` spans the next node's line, which sits no deeper than `owner`, and a
        // root body at column 0 could fold into a `---` or `...` marker.
        if folded && content <= owner.unwrap_or(0) {
            continue;
        }
        let indent = if folded {
            content
        } else {
            let existing = lines[first + 1..=last]
                .iter()
                .find(|(_, text)| !text.trim().is_empty())
                .map(|(_, text)| text.len() - text.trim_start_matches(' ').len())
                .filter(|spaces| *spaces > 0);
            if existing.is_some_and(|spaces| spaces < owner.unwrap_or(0) + min_step) {
                continue;
            }
            existing.unwrap_or_else(|| {
                owner.unwrap_or(0) + usize::from(cfg.indent).max(min_step)
            })
        };
        let continuation = format!("{newline}{}", " ".repeat(indent));
        for (index, (start, text)) in
            lines.iter().enumerate().take(last + 1).skip(first)
        {
            let scalar = if folded {
                let at_content = text
                    .get(..indent)
                    .is_some_and(|lead| lead.trim_start_matches(' ').is_empty())
                    && text[indent..].starts_with(|ch| !matches!(ch, ' ' | '\t'));
                if !at_content {
                    continue;
                }
                indent..text.len()
            } else {
                span.start.saturating_sub(*start)..span.end - start
            };
            let directive =
                text.get(scalar.end..).and_then(|rest| rest.split_once('#'));
            if directives.is_disabled(ID, index + 1)
                || directive
                    .is_some_and(|(_, comment)| directive_scope(comment).is_some())
            {
                continue;
            }
            for space in breaks(text, &scalar, style, indent, usize::from(cfg.width)) {
                edits.push((
                    BytePos::new(start + space),
                    BytePos::new(start + space + 1),
                    continuation.clone(),
                ));
            }
        }
    }
    (!edits.is_empty()).then(|| apply_replacements(buffer, edits))
}

/// The content (inside any quotes) of each block-context plain, quoted or `>` scalar that is
/// not a key, with the column of the collection that owns it (`None` at the document root)
/// and its style.
pub(crate) fn foldable_scalars(input: Analysis<'_>) -> Vec<FoldableScalar> {
    if let Some(facts) = input.layout_facts() {
        return facts.folds.clone();
    }
    let mut scalars = Foldables::default();
    for (span, kind) in input.tokens().iter() {
        scalars.push(span, kind);
    }
    scalars.finish()
}

#[derive(Default)]
pub(crate) struct Foldables {
    blocks: Vec<(usize, usize)>,
    flow: usize,
    owner: Option<Owner>,
    scalars: Vec<FoldableScalar>,
}

#[derive(Clone, Copy)]
enum Owner {
    Root,
    Collection(usize, usize),
}

impl Foldables {
    pub(crate) fn finish(self) -> Vec<FoldableScalar> {
        self.scalars
    }

    pub(crate) fn push(&mut self, span: granit_parser::Span, kind: &Kind) {
        self.owner = match kind {
            Kind::Anchor | Kind::Tag | Kind::Comment { .. } => {
                return;
            }
            Kind::StreamStart | Kind::DocumentStart | Kind::DocumentEnd => {
                Some(Owner::Root)
            }
            Kind::Value | Kind::BlockEntry => {
                Some(self.blocks.last().map_or(Owner::Root, |&(line, column)| {
                    Owner::Collection(line, column)
                }))
            }
            Kind::BlockMappingStart | Kind::BlockSequenceStart => {
                self.blocks.push((span.start.line() - 1, span.start.col()));
                None
            }
            Kind::BlockEnd => {
                self.blocks.pop();
                None
            }
            Kind::FlowMappingStart | Kind::FlowSequenceStart => {
                self.flow += 1;
                None
            }
            Kind::FlowMappingEnd | Kind::FlowSequenceEnd => {
                self.flow = self.flow.saturating_sub(1);
                None
            }
            Kind::Scalar { style, .. } if self.flow == 0 => {
                let quotes = usize::from(matches!(
                    style,
                    ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted
                ));
                if let Some(owner) = self.owner
                    && *style != ScalarStyle::Literal
                {
                    self.scalars.push(FoldableScalar {
                        content: marker_byte_offset(span.start).get() + quotes
                            ..marker_byte_offset(span.end).get() - quotes,
                        owner: match owner {
                            Owner::Root => None,
                            Owner::Collection(line, column) => Some((line, column)),
                        },
                        style: *style,
                    });
                }
                None
            }
            _ => None,
        };
    }
}

/// The byte offsets in `line` of the spaces to break at so each piece fits `width` chars
/// where it can, each continuation starting at column `indent`. A break is a lone space
/// inside `scalar` (a byte range of `line`), not escaped by an odd run of `\` in a
/// double-quoted `style`, and must shorten the line it splits.
fn breaks(
    line: &str,
    scalar: &Range<usize>,
    style: ScalarStyle,
    indent: usize,
    width: usize,
) -> Vec<usize> {
    breaks_from_chars(line.char_indices(), scalar, style, indent, width)
}

pub(crate) fn breaks_from_chars(
    chars: impl Iterator<Item = (usize, char)>,
    scalar: &Range<usize>,
    style: ScalarStyle,
    indent: usize,
    width: usize,
) -> Vec<usize> {
    let chars: Vec<(usize, char)> = chars.collect();
    let candidates: Vec<(usize, usize)> = chars
        .windows(3)
        .enumerate()
        .filter(|(index, window)| {
            let ((before, left), (after, right)) = (window[0], window[2]);
            window[1].1 == ' '
                && !left.is_whitespace()
                && !right.is_whitespace()
                // `comments-indentation` re-indents a `#`-led line in a quoted scalar, never in `>` (#525).
                && (right != '#' || style == ScalarStyle::Folded)
                && scalar.contains(&before)
                && scalar.contains(&after)
                && !(style == ScalarStyle::DoubleQuoted
                    && chars[..index + 1].iter()
                        .rev()
                        .take_while(|(_, ch)| *ch == '\\')
                        .count()
                        % 2
                        == 1)
        })
        .map(|(column, window)| (window[1].0, column + 1))
        .collect();
    let mut rest = candidates.as_slice();
    let (mut origin, mut base) = (0, 0);
    let mut out = Vec::new();
    while base + chars.len() - origin > width {
        let column = |at: usize| base + at - origin;
        let Some(first) = rest.iter().position(|(_, at)| column(*at) >= indent) else {
            break;
        };
        let eligible = &rest[first..];
        let pick = eligible
            .iter()
            .rposition(|(_, at)| column(*at) <= width)
            .unwrap_or(0);
        let (byte, at) = eligible[pick];
        out.push(byte);
        (origin, base) = (at + 1, indent);
        rest = &eligible[pick + 1..];
    }
    out
}
