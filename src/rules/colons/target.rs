use super::{Config, input::Initial};
use crate::format::layout::Permission;
use crate::rules::support::span_utils::BytePos;

pub(crate) struct RetainedKey(super::input::Key);

impl RetainedKey {
    pub(super) const fn new(initial: super::input::Key) -> Self {
        Self(initial)
    }
    pub(crate) const fn present(&self) -> bool {
        self.0.present
    }
    pub(crate) const fn separated(&self) -> bool {
        self.0.separated
    }
    pub(crate) const fn at(&self) -> BytePos {
        BytePos::new(self.0.at)
    }
    pub(crate) fn after(&self) -> BytePos {
        BytePos::new(self.0.at + usize::from(self.0.present))
    }
}

pub(crate) struct Guarantees<'a> {
    spaced: bool,
    permission: Permission<'a>,
}

impl<'a> Guarantees<'a> {
    pub(crate) fn new(config: Option<Config>, permission: Permission<'a>) -> Self {
        Self {
            spaced: config == Some(Config::format()),
            permission,
        }
    }

    fn spaces(&self, at: usize) -> bool {
        self.spaced
            && self
                .permission
                .allows(BytePos::new(at), BytePos::new(at + 1))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Context {
    follows: bool,
    adjacent: bool,
}

impl Context {
    pub(super) fn select(
        initial: Initial<'_>,
        end: Option<usize>,
        key_end: Option<usize>,
        guarantees: Option<&Guarantees<'_>>,
    ) -> Self {
        let spaced = |at| guarantees.is_some_and(|guarantees| guarantees.spaces(at));
        let follows = end
            .and_then(|end| initial.following(end))
            .is_some_and(|(at, gap)| gap == 0 || spaced(at));
        let adjacent =
            key_end
                .and_then(|end| initial.adjacent(end))
                .is_some_and(|(at, next)| {
                    !next.is_whitespace()
                        && (matches!(next, ',' | ']' | '}' | '#') || !spaced(at))
                });
        Self { follows, adjacent }
    }

    pub(crate) const fn follows(self) -> bool {
        self.follows
    }

    pub(crate) const fn adjacent(self) -> bool {
        self.adjacent
    }
}
