use crate::rules::quoted_strings::{ContentKind as Content, LayoutStyle};

use super::Fallback;
mod input;
pub(super) use input::SelectedLine;

pub(super) fn selected_line<'a>(
    geometry: &AllowedLine,
    parts: Vec<super::pieces::Piece<'a>>,
) -> SelectedLine<'a> {
    SelectedLine::new(parts, geometry.0.prefix, geometry.0.bom)
}
use crate::rules::line_length::Fold;
use crate::rules::line_length::fold::breaks_from_chars;

pub(super) struct ScalarCandidate {
    pub(super) preference: Option<crate::format::expression::Selected>,
    pub(super) style: LayoutStyle,
    pub(super) first: usize,
    pub(super) last: usize,
    pub(super) content: usize,
    pub(super) owner: usize,
    pub(super) existing: Option<usize>,
}

pub(super) struct ScalarTarget {
    preference: Option<crate::format::expression::Selected>,
    style: LayoutStyle,
    first: usize,
    last: usize,
    content: usize,
    indent: usize,
    width: u16,
}

pub(super) struct Geometry {
    pub(super) index: usize,
    pub(super) prefix: usize,
    pub(super) retained_prefix: usize,
    pub(super) bom: bool,
    pub(super) content: Content,
    pub(super) allowed: bool,
    pub(super) guarded_directive_tail: bool,
}

pub(super) struct Line<'a> {
    pub(super) geometry: AllowedLine,
    pub(super) content: SelectedLine<'a>,
    pub(super) scalar_end: Option<usize>,
    pub(super) ends_in_break: bool,
    pub(super) directive_tail: bool,
}

pub(super) struct AllowedLine(Geometry, bool);

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
        let folded = self.style.is_folded();
        let step = if self.style.is_plain() { 1 } else { 2 };
        if (!folded
            && self
                .existing
                .is_some_and(|indent| indent < self.owner + step))
            || (folded && self.content <= self.owner)
        {
            return None;
        }
        Some(ScalarTarget {
            preference: self.preference,
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
        !self.style.is_folded() && index == self.last
    }

    pub(super) fn allow_line(&self, line: Geometry) -> Option<AllowedLine> {
        let allowed = line.allowed
            && !(line.index == self.last && line.guarded_directive_tail)
            && (!self.style.is_folded()
                || (line.prefix == self.indent && line.content == Content::Text));
        (allowed || self.preference.is_some()).then_some(AllowedLine(line, allowed))
    }

    pub(super) fn plan_line(
        &mut self,
        line: Line<'_>,
    ) -> Result<impl Iterator<Item = FoldBreak> + use<>, Fallback> {
        let allowed = line.geometry.1;
        let geometry = line.geometry.0;
        let length = line.content.bytes();
        let start = if self.style.is_folded() {
            self.indent
        } else if geometry.index == self.first {
            self.content
        } else {
            0
        };
        let end = if self.needs_end(geometry.index) {
            line.scalar_end.ok_or(Fallback::ScalarInteraction)
        } else {
            Ok(length)
        };
        end.map(|end| {
            let preferred = self.preference.as_mut().map(|preference| {
                let text: String =
                    line.content.characters().map(|(_, ch)| ch).collect();
                preference.boundaries(&text, &(start..end), self.style)
            });
            // Byte length bounds character length, so short lines cannot need new breaks.
            let width = usize::from(self.width).saturating_sub(
                geometry.retained_prefix.saturating_sub(geometry.prefix),
            );
            let breaks = if !allowed
                || length <= width
                || (geometry.index == self.last
                    && !line.ends_in_break
                    && line.directive_tail)
            {
                Vec::new()
            } else {
                let chars = line.content.characters();
                breaks_from_chars(
                    chars,
                    &(start..end),
                    self.style,
                    self.indent,
                    width,
                    preferred.as_deref(),
                )
            };
            let line = geometry.index;
            let indent = self.indent;
            breaks
                .into_iter()
                .map(move |byte| FoldBreak { line, byte, indent })
        })
    }
}
