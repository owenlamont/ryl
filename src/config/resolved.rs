use std::any::Any;
use std::sync::OnceLock;

use crate::rules::ALL_RULE_IDS;

type Slot = OnceLock<Box<dyn Any + Send + Sync>>;

/// Facts derived from a config on first use. A clone starts empty, so a copy the caller
/// goes on to mutate never reuses facts derived from the original.
pub(super) struct Resolved {
    rules: [Slot; ALL_RULE_IDS.len()],
    has_path_ignores: OnceLock<bool>,
}

impl Resolved {
    pub(super) fn rule<C: Any + Send + Sync>(
        &self,
        rule: &str,
        resolve: impl FnOnce() -> C,
    ) -> &C {
        let slot = ALL_RULE_IDS
            .iter()
            .position(|id| *id == rule)
            .expect("every linted rule is listed in ALL_RULE_IDS");
        self.rules[slot]
            .get_or_init(|| Box::new(resolve()))
            .downcast_ref()
            .expect("a rule's slot only ever holds that rule's config type")
    }

    pub(super) fn has_path_ignores(&self, derive: impl FnOnce() -> bool) -> bool {
        *self.has_path_ignores.get_or_init(derive)
    }
}

impl Default for Resolved {
    fn default() -> Self {
        Self {
            rules: std::array::from_fn(|_| OnceLock::new()),
            has_path_ignores: OnceLock::new(),
        }
    }
}

impl Clone for Resolved {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Resolved")
    }
}
