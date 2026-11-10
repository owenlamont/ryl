use std::any::Any;
use std::sync::OnceLock;

use crate::rules::ALL_RULE_IDS;

type Slot = OnceLock<Box<dyn Any + Send + Sync>>;

/// Lazy rule facts reset on cloning because callers may mutate the copy.
pub(super) struct Resolved {
    pub(super) schema: Option<std::sync::Arc<crate::rules::schema::Prepared>>,
    pub(super) needs_remote_preparation: bool,
    pub(super) schema_request:
        Option<std::sync::Arc<crate::rules::schema::directives::Request>>,
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

    pub(super) fn reset_rule_facts(&mut self) {
        *self = Self {
            schema: self.schema.take(),
            schema_request: self.schema_request.take(),
            needs_remote_preparation: self.needs_remote_preparation,
            ..Self::default()
        };
    }
}

impl Default for Resolved {
    fn default() -> Self {
        Self {
            schema: None,
            needs_remote_preparation: false,
            schema_request: None,
            rules: std::array::from_fn(|_| OnceLock::new()),
            has_path_ignores: OnceLock::new(),
        }
    }
}

impl Clone for Resolved {
    fn clone(&self) -> Self {
        Self {
            schema: self.schema.clone(),
            schema_request: self.schema_request.clone(),
            needs_remote_preparation: self.needs_remote_preparation,
            ..Self::default()
        }
    }
}

impl std::fmt::Debug for Resolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(request) = &self.schema_request {
            return f
                .debug_tuple("Resolved")
                .field(&self.schema.as_ref().map(|s| s.digest))
                .field(request)
                .finish();
        }
        match &self.schema {
            Some(schema) => f
                .debug_tuple("Resolved")
                .field(&schema.digest)
                .field(&schema.bytes)
                .field(&schema.resources)
                .finish(),
            None => f.write_str("Resolved"),
        }
    }
}
