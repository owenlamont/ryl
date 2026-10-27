pub(super) use crate::rules::support::punctuation::Gap;

use super::{
    AfterResult, compute_spaces_after_open, compute_spaces_before_close,
    next_significant_index,
};

pub(super) struct Opening {
    empty: bool,
    gap: Option<Gap>,
    immediately_closed: bool,
}

impl Opening {
    pub(super) fn new(bytes: &[u8], at: usize, close: u8) -> Self {
        let empty =
            next_significant_index(bytes, at).is_some_and(|next| bytes[next] == close);
        let (gap, immediately_closed) = match compute_spaces_after_open(bytes, at) {
            AfterResult::SameLine { spaces, next_idx } => (
                Some(Gap::new(at + 1, next_idx, spaces)),
                bytes[next_idx] == close,
            ),
            AfterResult::Ignored => (None, false),
        };
        Self {
            empty,
            gap,
            immediately_closed,
        }
    }

    pub(super) const fn empty(&self) -> bool {
        self.empty
    }

    pub(super) const fn immediately_closed(&self) -> bool {
        self.immediately_closed
    }

    pub(super) fn into_gap(self) -> Option<Gap> {
        self.gap
    }
}

pub(super) struct Closing<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Closing<'a> {
    pub(super) const fn new(bytes: &'a [u8], at: usize) -> Self {
        Self { bytes, at }
    }

    pub(super) fn gap(self) -> Option<Gap> {
        compute_spaces_before_close(self.bytes, self.at)
            .map(|(spaces, start)| Gap::new(start, self.at, spaces))
    }
}
