//! Shared definition of a YAML merge key (`<<`).

use std::borrow::Cow;

use granit_parser::{ScalarStyle, Tag};

const MERGE_TAG: &str = "tag:yaml.org,2002:merge";

/// Two forms merge: an untagged plain `<<`, or ANY scalar explicitly tagged as the
/// merge type whatever its text (`!!merge foo` merges like `!!merge "<<"`, verified
/// against `PyYAML` and ruamel.yaml). A quoted `"<<"` or a `<<` with any other tag is
/// an ordinary string key.
#[must_use]
pub(crate) fn is_merge_directive(
    value: &str,
    style: ScalarStyle,
    tag: Option<&Cow<'_, Tag>>,
) -> bool {
    match tag {
        // Match the complete resolved URI (`handle` ++ `suffix`), not the canonical
        // split `core_schema_suffix` reports: a `%TAG` directive can split the URI
        // anywhere (YAML 1.2.2 6.8.2.2), so `!m!erge`, verbatim
        // `!<tag:yaml.org,2002:merge>`, and `!!merge` must all resolve alike.
        Some(tag) => MERGE_TAG
            .strip_prefix(tag.handle.as_str())
            .is_some_and(|suffix| suffix == tag.suffix.as_str()),
        None => value == "<<" && matches!(style, ScalarStyle::Plain),
    }
}
