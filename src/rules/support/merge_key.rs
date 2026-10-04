//! Shared definition of a YAML merge key (`<<`).

use std::borrow::Cow;

use granit_parser::{ScalarStyle, Tag};

use crate::yaml_dom::core_schema_suffix;

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
        Some(tag) => core_schema_suffix(tag).as_deref() == Some("merge"),
        None => value == "<<" && matches!(style, ScalarStyle::Plain),
    }
}
