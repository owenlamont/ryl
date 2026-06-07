//! Shared definition of a YAML merge key (`<<`), used by `key-duplicates`
//! (merge-collision detection) and `merge-keys` (portability).

use std::borrow::Cow;

use granit_parser::{ScalarStyle, Tag};

/// Whether a mapping key resolves to the YAML merge type (`tag:yaml.org,2002:merge`).
///
/// Two forms merge: an untagged plain `<<` (implicit resolution), or ANY scalar
/// explicitly tagged `!!merge` regardless of its text — `!!merge foo` merges in
/// `PyYAML` and ruamel.yaml exactly like `!!merge "<<"` (both verified). A quoted
/// `"<<"`, or a `<<` carrying any other tag, is an ordinary string key that never
/// merges.
#[must_use]
pub(crate) fn is_merge_directive(
    value: &str,
    style: ScalarStyle,
    tag: Option<&Cow<'_, Tag>>,
) -> bool {
    match tag {
        Some(tag) => tag.is_yaml_core_schema_tag("merge"),
        None => value == "<<" && matches!(style, ScalarStyle::Plain),
    }
}
