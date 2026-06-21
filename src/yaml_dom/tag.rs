//! Spelling-independent inspection of the `tag:yaml.org,2002:` namespace, wider
//! than granit's strict Core Schema accessors so `tags`/`key-duplicates` also see
//! the non-core types (`merge`, the removed YAML 1.1 types). It matches the full
//! resolved tag URI, so no verbatim or `%TAG`-split spelling can hide a type from
//! a check.

use std::borrow::Cow;

use granit_parser::Tag;

/// The `tag:yaml.org,2002:` namespace prefix that every spelling resolves to.
const CORE_SCHEMA_PREFIX: &str = "tag:yaml.org,2002:";

/// The `tag:yaml.org,2002:` type suffix this tag resolves to in any spelling, or
/// `None` outside the namespace. Wider than the Core Schema: also reports `merge`
/// and the removed YAML 1.1 types.
#[must_use]
pub fn core_schema_suffix(tag: &Tag) -> Option<Cow<'_, str>> {
    // A tag resolves to `handle ++ suffix` (YAML 1.2.2 §6.8.2.2) and a `%TAG`
    // directive can cut that URI at any point, so strip the namespace prefix
    // across the seam — otherwise a split spelling could hide a type from a check.
    if let Some(head) = tag.handle.strip_prefix(CORE_SCHEMA_PREFIX) {
        // The handle holds the whole prefix; the type is its tail plus the suffix
        // (only a mid-token `%TAG` split leaves a non-empty tail and must own it).
        return Some(if head.is_empty() {
            Cow::Borrowed(tag.suffix.as_str())
        } else {
            Cow::Owned(format!("{head}{}", tag.suffix))
        });
    }
    // The handle stops inside the prefix (an empty verbatim handle is one case);
    // the suffix must supply the prefix remainder before the type name.
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
