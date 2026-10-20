use crate::rules::support::span_utils::BytePos;
use crate::text_version::edits::Edit;

pub(super) type PlanEdit = (BytePos, BytePos, Replacement);

#[derive(PartialEq, Eq)]
pub(super) enum Replacement {
    Text(String),
    Spaces(usize),
}

impl Replacement {
    pub(super) fn piece(&self) -> Piece<'_> {
        match self {
            Self::Text(text) => Piece::Text(text),
            Self::Spaces(count) => Piece::Spaces(*count),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.piece().len()
    }
}

pub(super) fn plan_edit((start, end, text): Edit) -> PlanEdit {
    let replacement = if text.bytes().all(|byte| byte == b' ') {
        Replacement::Spaces(text.len())
    } else {
        Replacement::Text(text)
    };
    (start, end, replacement)
}

#[derive(Clone, Copy)]
pub(super) enum Piece<'a> {
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

    pub(super) fn chars(self) -> impl Iterator<Item = char> + 'a {
        let (spaces, text) = match self {
            Self::Text(text) => (0, text),
            Self::Spaces(count) => (count, ""),
        };
        std::iter::repeat_n(' ', spaces).chain(text.chars())
    }

    pub(super) fn is_char_boundary(self, at: usize) -> bool {
        match self {
            Self::Text(text) => text.is_char_boundary(at),
            Self::Spaces(count) => at <= count,
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
