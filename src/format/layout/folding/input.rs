use crate::format::layout::pieces::Piece;
use std::ops::Range;

pub(in crate::format::layout) struct SelectedLine<'a> {
    parts: Vec<Piece<'a>>,
    prefix: usize,
    bom: &'static str,
}

impl<'a> SelectedLine<'a> {
    pub(super) const fn new(parts: Vec<Piece<'a>>, prefix: usize, bom: bool) -> Self {
        Self {
            parts,
            prefix,
            bom: if bom { "\u{feff}" } else { "" },
        }
    }
    pub(super) fn bytes(&self) -> usize {
        self.prefix
            + self.bom.len()
            + self.parts.iter().map(|part| part.len()).sum::<usize>()
    }
    pub(super) fn scalar_characters(
        &self,
        scalar: Range<usize>,
    ) -> impl Iterator<Item = (usize, char)> + '_ {
        let mut byte = 0;
        self.bom
            .chars()
            .chain(std::iter::repeat_n(' ', self.prefix))
            .chain(self.parts.iter().flat_map(|part| part.chars()))
            .map(move |ch| {
                let at = byte;
                byte += ch.len_utf8();
                (at, if scalar.contains(&at) { ch } else { '\0' })
            })
    }
}
