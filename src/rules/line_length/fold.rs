use std::ops::Range;

use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};

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

/// `buffer` with each over-long line of a plain or quoted scalar in block context split
/// at single spaces, or `None` when nothing folds. A line `line-length` is disabled on
/// stays whole, as does one ending in a directive comment, which a fold would move.
#[must_use]
pub fn fold(buffer: &str, cfg: Fold) -> Option<String> {
    let directives = Directives::parse(buffer);
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
    for (span, owner, style) in foldable_scalars(buffer) {
        // granit rejects a quoted mapping value continued one column past its key.
        let min_step = if style == ScalarStyle::Plain { 1 } else { 2 };
        let (first, last) = (line_of(span.start), line_of(span.end - 1));
        let indent = lines[first + 1..=last]
            .iter()
            .find(|(_, text)| !text.trim().is_empty())
            .map(|(_, text)| text.len() - text.trim_start_matches(' ').len())
            .filter(|spaces| *spaces > 0)
            .unwrap_or_else(|| {
                owner.unwrap_or(0) + usize::from(cfg.indent).max(min_step)
            });
        let continuation = format!("{newline}{}", " ".repeat(indent));
        for (index, (start, text)) in
            lines.iter().enumerate().take(last + 1).skip(first)
        {
            let scalar = span.start.saturating_sub(*start)..span.end - start;
            let directive =
                text.get(scalar.end..).and_then(|rest| rest.split_once('#'));
            if directives.is_disabled(ID, index + 1)
                || directive
                    .is_some_and(|(_, comment)| directive_scope(comment).is_some())
            {
                continue;
            }
            for space in breaks(
                text,
                &scalar,
                style == ScalarStyle::DoubleQuoted,
                indent,
                usize::from(cfg.width),
            ) {
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

/// The content (inside any quotes) of each block-context plain or quoted scalar that is not
/// a key, with the column of the collection that owns it (`None` at the document root)
/// and its style.
fn foldable_scalars(buffer: &str) -> Vec<(Range<usize>, Option<usize>, ScalarStyle)> {
    let mut blocks = Vec::new();
    let mut flow = 0usize;
    let mut owner = None;
    let mut scalars = Vec::new();
    for token in Scanner::new(StrInput::new(buffer)).map_while(Result::ok) {
        let (span, kind) = token.into_parts();
        owner = match kind {
            TokenType::Anchor(_) | TokenType::Tag(..) | TokenType::Comment(_) => {
                continue;
            }
            TokenType::StreamStart
            | TokenType::DocumentStart
            | TokenType::DocumentEnd => Some(None),
            TokenType::Value | TokenType::BlockEntry => Some(blocks.last().copied()),
            TokenType::BlockMappingStart | TokenType::BlockSequenceStart => {
                blocks.push(span.start.col());
                None
            }
            TokenType::BlockEnd => {
                blocks.pop();
                None
            }
            TokenType::FlowMappingStart | TokenType::FlowSequenceStart => {
                flow += 1;
                None
            }
            TokenType::FlowMappingEnd | TokenType::FlowSequenceEnd => {
                flow = flow.saturating_sub(1);
                None
            }
            TokenType::Scalar(style, _) if flow == 0 => {
                let quotes = usize::from(style != ScalarStyle::Plain);
                if let Some(owner) = owner
                    && !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
                {
                    scalars.push((
                        marker_byte_offset(span.start).get() + quotes
                            ..marker_byte_offset(span.end).get() - quotes,
                        owner,
                        style,
                    ));
                }
                None
            }
            _ => None,
        };
    }
    scalars
}

/// The byte offsets in `line` of the spaces to break at so each piece fits `width` chars
/// where it can, each continuation starting at column `indent`. A break is a lone space
/// inside `scalar` (a byte range of `line`), not escaped by an odd run of `\` when
/// `escapes`, and must shorten the line it splits.
fn breaks(
    line: &str,
    scalar: &Range<usize>,
    escapes: bool,
    indent: usize,
    width: usize,
) -> Vec<usize> {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let candidates: Vec<(usize, usize)> = chars
        .windows(3)
        .enumerate()
        .filter(|(_, window)| {
            let ((before, left), (after, right)) = (window[0], window[2]);
            window[1].1 == ' '
                && !left.is_whitespace()
                && !right.is_whitespace()
                // `comments-indentation` re-indents a `#`-led line even in a quoted scalar (#525).
                && right != '#'
                && scalar.contains(&before)
                && scalar.contains(&after)
                && !(escapes
                    && line[..window[1].0]
                        .bytes()
                        .rev()
                        .take_while(|byte| *byte == b'\\')
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
