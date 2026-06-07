//! Shared definition of a YAML merge key (`<<`), used by `key-duplicates`
//! (merge-collision detection) and `merge-keys` (portability).

use std::borrow::Cow;

use granit_parser::{ScalarStyle, Tag};

/// The fully-resolved YAML core-schema merge tag.
const MERGE_TAG: &str = "tag:yaml.org,2002:merge";

/// Whether a mapping key resolves to the YAML merge type (`tag:yaml.org,2002:merge`).
///
/// Two forms merge: an untagged plain `<<` (implicit resolution), or ANY scalar
/// explicitly tagged as the merge type regardless of its text — `!!merge foo`
/// merges in `PyYAML` and ruamel.yaml exactly like `!!merge "<<"` (both verified).
/// A quoted `"<<"`, or a `<<` carrying any other tag, is an ordinary string key
/// that never merges.
#[must_use]
pub(crate) fn is_merge_directive(
    value: &str,
    style: ScalarStyle,
    tag: Option<&Cow<'_, Tag>>,
) -> bool {
    match tag {
        // Match by resolved URI (`handle + suffix`) so shorthand (`!!merge`) and
        // verbatim (`!<tag:yaml.org,2002:merge>`, which granit scans to an empty
        // handle) are both recognised; `Tag::is_yaml_core_schema` inspects only
        // the handle and so misses the verbatim spelling.
        Some(tag) => MERGE_TAG
            .strip_prefix(tag.handle.as_str())
            .is_some_and(|suffix| suffix == tag.suffix.as_str()),
        None => value == "<<" && matches!(style, ScalarStyle::Plain),
    }
}
