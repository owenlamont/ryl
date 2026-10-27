use crate::text_version::model::Comment;

use super::Config;

#[derive(Default)]
pub(crate) struct Magic(bool);

impl Magic {
    pub(super) fn plan(
        initial: super::input::Magic<'_>,
        config: &Config,
        edits: &[crate::text_version::edits::Edit],
        permission: crate::format::layout::Permission<'_>,
    ) -> Self {
        Self(
            !config.ignore_shebangs()
                && initial.end().is_some_and(|end| {
                    edits.iter().any(|(start, to, _)| {
                        start.get() <= end && permission.allows(*start, *to)
                    })
                }),
        )
    }

    pub(crate) const fn changed(&self) -> bool {
        self.0
    }
}

#[derive(Clone, Copy)]
pub(super) struct Initial {
    pub(super) gap: Option<usize>,
}

#[derive(Clone, Copy)]
pub(super) enum Gap {
    Keep,
    Insert(usize),
    Replace { old: usize, target: usize },
}

pub(super) struct Target {
    gap: Gap,
    starting_space: Option<usize>,
}

impl Target {
    pub(super) fn plan(model: Comment<'_>, initial: Initial, cfg: &Config) -> Self {
        let gap = initial.gap.map_or(Gap::Keep, |spacing| {
            match (cfg.min_spaces_from_content, cfg.max_spaces_from_content) {
                (Some(min), _) if spacing < min => Gap::Insert(min - spacing),
                (_, Some(max)) if spacing > max => Gap::Replace {
                    old: spacing,
                    target: max,
                },
                _ => Gap::Keep,
            }
        });
        let starting_space = if cfg.require_starting_space()
            && !(cfg.ignore_shebangs() && model.is_magic())
        {
            let hashes = model.value().bytes().take_while(|&ch| ch == b'#').count();
            model.value()[hashes..]
                .chars()
                .next()
                .filter(|&ch| ch != ' ')
                .map(|_| hashes)
        } else {
            None
        };
        Self {
            gap,
            starting_space,
        }
    }

    pub(super) fn gap(&self) -> Gap {
        self.gap
    }

    pub(super) fn starting_space(&self) -> Option<usize> {
        self.starting_space
    }
}
