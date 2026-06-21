//! Spelling-independent inspection of the `tag:yaml.org,2002:` namespace. Wider than
//! granit's strict Core Schema accessors so `tags`/`key-duplicates` also see non-core
//! types (`merge`, removed YAML 1.1 types); matches the full resolved URI so no
//! verbatim or `%TAG`-split spelling can hide a type.

use std::borrow::Cow;

use granit_parser::Tag;

const CORE_SCHEMA_PREFIX: &str = "tag:yaml.org,2002:";

/// The namespace type suffix `tag` resolves to in any spelling, or `None` outside it.
#[must_use]
pub fn core_schema_suffix(tag: &Tag) -> Option<Cow<'_, str>> {
    // A tag resolves to `handle ++ suffix` (YAML 1.2.2 6.8.2.2) and a `%TAG` directive
    // can cut the URI at any point, so strip the prefix across the seam; otherwise a
    // split spelling could hide a type.
    if let Some(head) = tag.handle.strip_prefix(CORE_SCHEMA_PREFIX) {
        // Handle holds the whole prefix; the type is its tail plus the suffix (only a
        // mid-token split leaves a non-empty tail).
        return Some(if head.is_empty() {
            Cow::Borrowed(tag.suffix.as_str())
        } else {
            Cow::Owned(format!("{head}{}", tag.suffix))
        });
    }
    // Handle stops inside the prefix; the suffix supplies the remainder.
    CORE_SCHEMA_PREFIX
        .strip_prefix(tag.handle.as_str())
        .and_then(|prefix_tail| tag.suffix.strip_prefix(prefix_tail))
        .map(Cow::Borrowed)
}

/// Whether `tag` resolves into the `tag:yaml.org,2002:` namespace in any spelling.
#[must_use]
pub fn is_core_schema(tag: &Tag) -> bool {
    core_schema_suffix(tag).is_some()
}
