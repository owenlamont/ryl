use std::any::Any;
use std::sync::OnceLock;

use crate::rules::ALL_RULE_IDS;

type Slot = OnceLock<Box<dyn Any + Send + Sync>>;

/// Lazy rule facts reset on cloning because callers may mutate the copy.
pub(super) struct Resolved {
    pub(super) schema: Option<std::sync::Arc<crate::rules::schema::Prepared>>,
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
            schema: None,
            rules: std::array::from_fn(|_| OnceLock::new()),
            has_path_ignores: OnceLock::new(),
        }
    }
}

impl Clone for Resolved {
    fn clone(&self) -> Self {
        Self {
            schema: self.schema.clone(),
            ..Self::default()
        }
    }
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.schema {
            Some(schema) => f.debug_tuple("Resolved").field(&schema.digest).finish(),
            None => f.write_str("Resolved"),
        }
    }
}
