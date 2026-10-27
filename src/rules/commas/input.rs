use super::{AfterResult, BeforeResult, compute_spaces_after, compute_spaces_before};
pub(super) use crate::rules::support::punctuation::Gap;

#[derive(Clone, Copy)]
pub(super) struct Initial<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Initial<'a> {
    pub(super) const fn new(bytes: &'a [u8], at: usize) -> Self {
        Self { bytes, at }
    }

    pub(super) fn before(self) -> Option<Gap> {
        match compute_spaces_before(self.bytes, self.at) {
            BeforeResult::SameLine { spaces, start_idx } => {
                Some(Gap::new(start_idx, self.at, spaces))
            }
            BeforeResult::Ignored => None,
        }
    }

    pub(super) fn after(self) -> Option<Gap> {
        match compute_spaces_after(self.bytes, self.at) {
            AfterResult::SameLine { spaces, next_char } => {
                Some(Gap::new(self.at + 1, next_char, spaces))
            }
            AfterResult::Ignored => None,
        }
    }
}
