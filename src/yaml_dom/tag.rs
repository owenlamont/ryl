//! Spelling-independent inspection of the `tag:yaml.org,2002:` namespace. Wider than
//! granit's strict Core Schema accessors so `tags`/`key-duplicates` also see non-core
//! types (`merge`, removed YAML 1.1 types).

use std::borrow::Cow;

use granit_parser::Tag;

/// The namespace type suffix `tag` resolves to in any spelling, or `None` outside it.
#[must_use]
pub fn core_schema_suffix(tag: &Tag) -> Option<Cow<'_, str>> {
    tag.suffix_in_namespace("tag:yaml.org,2002:")
}

/// Whether `tag` resolves into the `tag:yaml.org,2002:` namespace in any spelling.
#[must_use]
pub fn is_core_schema(tag: &Tag) -> bool {
    core_schema_suffix(tag).is_some()
}
