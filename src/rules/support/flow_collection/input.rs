pub(super) use crate::rules::support::punctuation::Gap;
use crate::rules::support::punctuation::{AfterResult, compute_spaces_after};

use super::{compute_spaces_before_close, next_significant_index};

pub(super) struct Opening {
    empty: bool,
    gap: Option<Gap>,
    immediately_closed: bool,
}

impl Opening {
    pub(super) fn new(bytes: &[u8], at: usize, close: u8) -> Self {
        let empty =
            next_significant_index(bytes, at).is_some_and(|next| bytes[next] == close);
        let (gap, immediately_closed) = match compute_spaces_after(bytes, at) {
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

pub(crate) enum CollectionKind {
    Mapping,
    Sequence,
}

pub(crate) struct RetainedClosing<'a> {
    bytes: &'a [u8],
    comma: usize,
    end: Option<usize>,
}

impl<'a> RetainedClosing<'a> {
    pub(super) const fn new(bytes: &'a [u8], comma: usize, end: Option<usize>) -> Self {
        Self { bytes, comma, end }
    }

    pub(crate) fn kind(&self) -> Option<CollectionKind> {
        self.end.and_then(|end| match self.bytes.get(end) {
            Some(b'}') => Some(CollectionKind::Mapping),
            Some(b']') => Some(CollectionKind::Sequence),
            _ => None,
        })
    }

    pub(crate) fn follows(&self) -> bool {
        let mut from = self.comma + 1;
        loop {
            match self.bytes.get(from) {
                Some(b' ' | b'\t' | b'\n' | b'\r') => from += 1,
                Some(b'#') => {
                    from += self.bytes[from..]
                        .iter()
                        .position(|&byte| matches!(byte, b'\n' | b'\r'))
                        .unwrap_or(self.bytes.len() - from);
                }
                next => return matches!(next, Some(b']' | b'}')),
            }
        }
    }
}

pub(super) struct Traversal<'a> {
    buffer: &'a str,
    sequence: bool,
}

impl<'a> Traversal<'a> {
    pub(super) const fn new(
        buffer: &'a str,
        descriptor: &super::FlowCollectionDescriptor,
    ) -> Self {
        Self {
            buffer,
            sequence: descriptor.open == b'[',
        }
    }
    pub(super) fn templated(&self) -> bool {
        self.sequence && self.buffer.contains("{{")
    }
}
