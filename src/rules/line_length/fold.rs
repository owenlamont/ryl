use std::ops::Range;

use crate::text_version::Analysis;
use crate::text_version::lexical::Kind;
use ryl_yaml_parser::ScalarStyle;

use super::Config;
use crate::rules::support::span_utils::marker_byte_offset;

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

/// The content (inside any quotes) of each block-context plain, quoted or `>` scalar that is
/// not a key, with the column of the collection that owns it (`None` at the document root)
/// and its style.
pub(crate) fn foldable_scalars(input: Analysis<'_>) -> Vec<FoldableScalar> {
    input
        .layout_facts()
        .expect("layout facts are seeded before fold planning")
        .folds
        .clone()
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

    pub(crate) fn push(&mut self, span: ryl_yaml_parser::Span, kind: &Kind) {
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
