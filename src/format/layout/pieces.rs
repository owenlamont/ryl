use crate::rules::support::span_utils::BytePos;
use crate::text_version::edits::Edit;

pub(crate) type PlanEdit = (BytePos, BytePos, Replacement);

#[derive(PartialEq, Eq)]
pub(crate) enum Replacement {
    Text { text: String, chars: usize },
    Spaces(usize),
}

impl Replacement {
    pub(crate) fn piece(&self) -> Piece<'_> {
        match self {
            Self::Text { text, .. } => Piece::Text(text),
            Self::Spaces(count) => Piece::Spaces(*count),
        }
    }

    pub(crate) fn char_count(&self) -> usize {
        match self {
            Self::Text { chars, .. } => *chars,
            Self::Spaces(count) => *count,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.piece().len()
    }
}

pub(super) fn plan_edit((start, end, text): Edit) -> PlanEdit {
    let replacement = if text.bytes().all(|byte| byte == b' ') {
        Replacement::Spaces(text.len())
    } else {
        Replacement::Text {
            chars: text.chars().count(),
            text,
        }
    };
    (start, end, replacement)
}

#[derive(Clone, Copy)]
pub(crate) enum Piece<'a> {
    Text(&'a str),
    Spaces(usize),
}

impl<'a> Piece<'a> {
    pub(super) fn len(self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Spaces(count) => count,
        }
    }

    pub(crate) fn chars(self) -> impl Iterator<Item = char> + 'a {
        let (spaces, text) = match self {
            Self::Text(text) => (0, text),
            Self::Spaces(count) => (count, ""),
        };
        std::iter::repeat_n(' ', spaces).chain(text.chars())
    }

    pub(super) fn get(self, range: std::ops::Range<usize>) -> Option<Self> {
        match self {
            Self::Text(text) => text.get(range).map(Self::Text),
            Self::Spaces(count) => (range.start <= range.end && range.end <= count)
                .then_some(Self::Spaces(range.len())),
        }
    }

    pub(super) fn slice(self, range: std::ops::Range<usize>) -> Self {
        match self {
            Self::Text(text) => Self::Text(&text[range]),
            Self::Spaces(_) => Self::Spaces(range.len()),
        }
    }

    pub(super) fn append(self, output: &mut String) {
        match self {
            Self::Text(text) => output.push_str(text),
            Self::Spaces(count) => output.extend(std::iter::repeat_n(' ', count)),
        }
    }
}

pub(super) fn bridge_width(
    bridge: Option<&super::Bridge>,
    byte: usize,
) -> Option<usize> {
    bridge.map_or(Some(0), |bridge| {
        (byte >= bridge.end).then_some(bridge.bytes)
    })
}
