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
