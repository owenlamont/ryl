use ryl_yaml_parser::ScalarStyle;

use super::Fallback;
use super::pieces::Piece;
use crate::rules::line_length::Fold;
use crate::rules::line_length::fold::breaks_from_chars;

pub(super) struct ScalarCandidate {
    pub(super) style: ScalarStyle,
    pub(super) first: usize,
    pub(super) last: usize,
    pub(super) content: usize,
    pub(super) owner: usize,
    pub(super) existing: Option<usize>,
}

pub(super) struct ScalarTarget {
    style: ScalarStyle,
    first: usize,
    last: usize,
    content: usize,
    indent: usize,
    width: u16,
}

pub(super) struct Geometry {
    pub(super) index: usize,
    pub(super) prefix: usize,
    pub(super) bom: bool,
    pub(super) content: Content,
    pub(super) allowed: bool,
    pub(super) guarded_directive_tail: bool,
}

#[derive(PartialEq, Eq)]
pub(super) enum Content {
    Empty,
    Whitespace,
    Text,
}

pub(super) struct Line<'a> {
    pub(super) geometry: AllowedLine,
    pub(super) parts: Vec<Piece<'a>>,
    pub(super) scalar_end: Option<usize>,
    pub(super) ends_in_break: bool,
    pub(super) directive_tail: bool,
}

pub(super) struct AllowedLine(Geometry);

pub(super) struct Tail {
    pub(super) ends_in_break: bool,
    pub(super) directive: bool,
    pub(super) guarded_directive: bool,
}

pub(super) struct FoldBreak {
    pub(super) line: usize,
    pub(super) byte: usize,
    pub(super) indent: usize,
}

impl ScalarCandidate {
    pub(super) fn settle(self, cfg: Fold) -> Option<ScalarTarget> {
        let folded = self.style == ScalarStyle::Folded;
        let step = if self.style == ScalarStyle::Plain {
            1
        } else {
            2
        };
        if (!folded
            && self
                .existing
                .is_some_and(|indent| indent < self.owner + step))
            || (folded && self.content <= self.owner)
        {
            return None;
        }
        Some(ScalarTarget {
            style: self.style,
            first: self.first,
            last: self.last,
            content: self.content,
            indent: if folded {
                self.content
            } else {
                self.existing
                    .unwrap_or(self.owner + usize::from(cfg.indent).max(step))
            },
            width: cfg.width,
        })
    }
}

impl ScalarTarget {
    pub(super) fn needs_end(&self, index: usize) -> bool {
        self.style != ScalarStyle::Folded && index == self.last
    }

    pub(super) fn allow_line(&self, line: Geometry) -> Option<AllowedLine> {
        (line.allowed
            && !(line.index == self.last && line.guarded_directive_tail)
            && (self.style != ScalarStyle::Folded
                || (line.prefix == self.indent && line.content == Content::Text)))
            .then_some(AllowedLine(line))
    }

    pub(super) fn plan_line(&self, line: Line<'_>) -> Result<Vec<FoldBreak>, Fallback> {
        let geometry = line.geometry.0;
        let bom = if geometry.bom { "\u{feff}" } else { "" };
        let length = geometry.prefix
            + bom.len()
            + line.parts.iter().map(|part| part.len()).sum::<usize>();
        let start = if self.style == ScalarStyle::Folded {
            self.indent
        } else if geometry.index == self.first {
            self.content
        } else {
            0
        };
        let end = if self.needs_end(geometry.index) {
            line.scalar_end.ok_or(Fallback::ScalarInteraction)?
        } else {
            length
        };
        // Byte length bounds character length, so short lines need no character walk.
        if length <= usize::from(self.width)
            || (geometry.index == self.last
                && !line.ends_in_break
                && line.directive_tail)
        {
            return Ok(Vec::new());
        }
        let chars = bom
            .chars()
            .chain(std::iter::repeat_n(' ', geometry.prefix))
            .chain(line.parts.iter().flat_map(|part| part.chars()));
        let mut byte = 0;
        let chars = chars.map(|ch| {
            let at = byte;
            byte += ch.len_utf8();
            (at, ch)
        });
        Ok(breaks_from_chars(
            chars,
            &(start..end),
            self.style,
            self.indent,
            usize::from(self.width),
        )
        .into_iter()
        .map(|byte| FoldBreak {
            line: geometry.index,
            byte,
            indent: self.indent,
        })
        .collect())
    }
}
