use super::input::MappingSite;
use super::{Kind, SpacesSetting, Token};
use crate::directives::Directives;
use crate::rules::support::collection_style::RetainedContext;
use crate::rules::{comments::RetainedComments, hyphens};
use crate::text_version::model::Mapping;

pub(crate) struct SplitTargets(Vec<usize>);

impl SplitTargets {
    pub(super) const fn new(starts: Vec<usize>) -> Self {
        Self(starts)
    }

    pub(crate) fn keeps_gap(
        &self,
        dash: &hyphens::RetainedDash,
        end: crate::rules::support::span_utils::BytePos,
    ) -> bool {
        !dash.present() || self.0.binary_search(&end.get()).is_err()
    }
}

pub(super) struct Candidate {
    recipe: Recipe,
    tight: bool,
}

pub(super) struct Recipe {
    line: usize,
    split: Option<usize>,
    dash: usize,
}

impl Recipe {
    pub(super) const fn line(&self) -> usize {
        self.line
    }

    pub(super) fn into_parts(self) -> (usize, Option<usize>, usize) {
        (self.line, self.split, self.dash)
    }
}

pub(super) struct TightJoins<'a> {
    spaces: SpacesSetting,
    permissions: &'a Directives,
}

impl<'a> TightJoins<'a> {
    pub(super) const fn new(
        spaces: SpacesSetting,
        permissions: &'a Directives,
    ) -> Self {
        Self {
            spaces,
            permissions,
        }
    }

    fn allowed(&self, line: usize) -> bool {
        match self.spaces {
            SpacesSetting::Fixed(1) => false,
            SpacesSetting::Fixed(2) => true,
            _ => [line, line + 1]
                .iter()
                .all(|&line| !self.permissions.is_disabled(hyphens::ID, line + 1)),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct MappingAdmission(());

impl MappingAdmission {
    pub(super) fn new(model: &Mapping, context: RetainedContext) -> Option<Self> {
        (model.in_sequence() && !model.has_properties() && !context.in_flow())
            .then_some(Self(()))
    }
}

pub(super) fn from_event(
    _admission: MappingAdmission,
    site: MappingSite,
    entry: &hyphens::EntrySite,
    own_line: bool,
) -> Option<Candidate> {
    let position = entry.inline().map(|dash| (site.line(), dash)).or_else(|| {
        let (dash, comment) = entry.above()?;
        (!comment.has_comment()).then_some((site.line() - 1, dash))
    });
    let (line, dash) = position?;
    if own_line && line == site.line() {
        Some(Candidate {
            recipe: Recipe {
                line,
                split: Some(site.column()),
                dash,
            },
            tight: false,
        })
    } else if !own_line && line + 1 == site.line() && site.column() > dash {
        Some(Candidate {
            recipe: Recipe {
                line,
                split: None,
                dash,
            },
            tight: site.column() == dash + 1,
        })
    } else {
        None
    }
}

pub(super) fn from_tokens(
    entry: &Token,
    start: &Token,
    gap: &hyphens::DashGap<'_>,
    own_line: bool,
    permissions: &Directives,
) -> Option<Candidate> {
    if entry.kind != Kind::BlockEntry
        || start.kind != Kind::BlockMappingStart
        || permissions.is_disabled(hyphens::ID, entry.line + 1)
        || permissions.is_disabled(hyphens::ID, start.line + 1)
    {
        return None;
    }
    if own_line && start.line == entry.line {
        Some(Candidate {
            recipe: Recipe {
                line: entry.line,
                split: Some(start.column),
                dash: entry.column,
            },
            tight: false,
        })
    } else if !own_line
        && start.line == entry.line + 1
        && start.column > entry.column + 1
        && gap.ends_at(entry.column + 1)
    {
        Some(Candidate {
            recipe: Recipe {
                line: entry.line,
                split: None,
                dash: entry.column,
            },
            tight: false,
        })
    } else {
        None
    }
}

pub(super) fn settle(
    candidates: Vec<Candidate>,
    comments: &RetainedComments,
    joins: &TightJoins<'_>,
    starts: &[usize],
) -> (Vec<Recipe>, Vec<(usize, usize)>) {
    let document = |line| starts.partition_point(|&at| at <= line);
    let refused: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.tight && !joins.allowed(candidate.recipe.line))
        .map(|candidate| document(candidate.recipe.line))
        .collect();
    let mut kept = Vec::new();
    let recipes = candidates
        .into_iter()
        .filter_map(|candidate| {
            let recipe = candidate.recipe;
            if recipe.split.is_some() && comments.line(recipe.line) {
                kept.push((recipe.line + 1, recipe.dash + 1));
                None
            } else if candidate.tight && refused.contains(&document(recipe.line)) {
                None
            } else {
                Some(recipe)
            }
        })
        .collect();
    (recipes, kept)
}

pub(super) struct Collapses {
    last: Option<usize>,
    compatible: bool,
}

impl Collapses {
    pub(super) const fn new() -> Self {
        Self {
            last: None,
            compatible: true,
        }
    }
    pub(super) fn add(&mut self, site: &super::input::Collapse) {
        self.compatible &= site.flat
            && site.first <= site.last
            && site.tails_empty
            && site.body.start <= site.replacement.start
            && site.replacement.start <= site.first_body_end
            && site.replacement.end <= site.body.end
            && self.last.is_none_or(|last| last < site.first);
        self.last = Some(site.last);
    }
    pub(super) const fn accepts(&self) -> bool {
        self.compatible
    }
}

pub(crate) fn discards_gap(
    site: &super::input::DiscardedGap<'_>,
    dash: &hyphens::RetainedDash,
) -> bool {
    !site.bridged
        && dash.present()
        && site.at_body_end
        && site.before_tail
        && site.whitespace()
}
