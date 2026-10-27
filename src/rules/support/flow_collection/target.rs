use super::{Config, Forbid, input};
use crate::rules::support::{punctuation::target_spacing, token_spacing::SpaceEdit};

pub(super) struct Planner<'a> {
    config: &'a Config,
    empty: Vec<bool>,
    replacements: Vec<SpaceEdit>,
}

impl<'a> Planner<'a> {
    pub(super) fn new(config: &'a Config) -> Option<Self> {
        (config.forbid() == Forbid::None).then(|| Self {
            config,
            empty: Vec::new(),
            replacements: Vec::new(),
        })
    }

    pub(super) fn open(&mut self, initial: input::Opening) {
        self.content();
        let mut empty = initial.empty();
        let immediately_closed = initial.immediately_closed();
        if let Some(gap) = initial.into_gap() {
            let (min, max) = if empty && immediately_closed {
                (
                    self.config.effective_min_empty(),
                    self.config.effective_max_empty(),
                )
            } else {
                empty = false;
                (
                    self.config.min_spaces_inside(),
                    self.config.max_spaces_inside(),
                )
            };
            let target = target_spacing(gap.spaces(), min, max);
            self.replacements.extend(gap.select(target));
        }
        self.empty.push(empty);
    }

    pub(super) fn close(&mut self, initial: input::Closing<'_>) {
        if let Some(false) = self.empty.pop()
            && let Some(gap) = initial.gap()
        {
            let target = target_spacing(
                gap.spaces(),
                self.config.min_spaces_inside(),
                self.config.max_spaces_inside(),
            );
            self.replacements.extend(gap.select(target));
        }
    }

    pub(super) fn content(&mut self) {
        if let Some(empty) = self.empty.last_mut() {
            *empty = false;
        }
    }

    pub(super) fn finish(self) -> Vec<SpaceEdit> {
        self.replacements
    }
}

pub(crate) struct ClosingPolicy<'a> {
    config: Option<&'a Config>,
    permission: crate::format::layout::Permission<'a>,
}

impl<'a> ClosingPolicy<'a> {
    pub(crate) fn new(
        config: Option<&'a Config>,
        permission: crate::format::layout::Permission<'a>,
    ) -> Self {
        Self { config, permission }
    }

    pub(crate) fn select(
        &self,
        edit: crate::rules::support::token_spacing::SpaceEdit,
    ) -> Option<usize> {
        let config = self.config.filter(|cfg| cfg.forbid() == Forbid::None)?;
        self.permission.allows(edit.0, edit.1).then(|| {
            target_spacing(
                edit.2,
                config.min_spaces_inside(),
                config.max_spaces_inside(),
            )
        })
    }
}

pub(super) enum Traversal {
    Flow,
    Template,
}

impl Traversal {
    pub(super) fn select(
        initial: &input::Traversal<'_>,
        config: &Config,
    ) -> Option<Self> {
        (config.forbid() == Forbid::None).then(|| {
            if initial.templated() {
                Self::Template
            } else {
                Self::Flow
            }
        })
    }
}
