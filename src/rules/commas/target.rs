use super::{Config, input::Initial};
use crate::rules::support::{punctuation::target_spacing, token_spacing::SpaceEdit};

pub(super) fn select(
    config: &Config,
    initial: Initial<'_>,
) -> impl Iterator<Item = SpaceEdit> + use<> {
    let before = initial.before().and_then(|gap| {
        let target = usize::try_from(config.max_spaces_before).ok()?;
        (gap.spaces() > target)
            .then(|| gap.select(target))
            .flatten()
    });
    let after = initial.after().and_then(|gap| {
        let target = target_spacing(
            gap.spaces(),
            config.min_spaces_after,
            config.max_spaces_after,
        );
        gap.select(target)
    });
    before.into_iter().chain(after)
}

pub(crate) struct Closing<'a> {
    config: Option<Config>,
    preserve: bool,
    mapping: crate::rules::support::flow_collection::ClosingPolicy<'a>,
    sequence: crate::rules::support::flow_collection::ClosingPolicy<'a>,
    permission: crate::format::layout::Permission<'a>,
}

impl<'a> Closing<'a> {
    pub(super) const fn new(
        config: Option<Config>,
        preserve: bool,
        mapping: crate::rules::support::flow_collection::ClosingPolicy<'a>,
        sequence: crate::rules::support::flow_collection::ClosingPolicy<'a>,
        permission: crate::format::layout::Permission<'a>,
    ) -> Self {
        Self {
            config,
            preserve,
            mapping,
            sequence,
            permission,
        }
    }

    pub(super) fn select(
        &self,
        initial: &crate::rules::support::flow_collection::RetainedClosing<'_>,
        edit: Option<&mut SpaceEdit>,
        closing: &mut Vec<(
            crate::rules::support::span_utils::BytePos,
            crate::rules::support::span_utils::BytePos,
        )>,
    ) -> bool {
        use crate::rules::support::flow_collection::CollectionKind;
        if self.preserve && initial.follows() {
            return true;
        }
        let Some(edit) = edit else {
            return false;
        };
        let policy = match initial.kind() {
            Some(CollectionKind::Mapping) => &self.mapping,
            Some(CollectionKind::Sequence) => &self.sequence,
            None => return false,
        };
        let Some(spaces) = policy.select(*edit) else {
            return false;
        };
        if !self.permission.allows(edit.0, edit.1) {
            return false;
        }
        edit.2 = spaces;
        let contradicts = self.config.is_some_and(|config| {
            target_spacing(spaces, config.min_spaces_after(), config.max_spaces_after())
                != spaces
        });
        if !contradicts {
            closing.push((edit.0, edit.1));
        }
        contradicts
    }
}
