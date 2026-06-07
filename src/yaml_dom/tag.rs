//! Spelling-independent inspection of YAML core-schema tags.
//!
//! `granit_parser::Tag::is_yaml_core_schema` inspects only the tag *handle*, so a
//! verbatim core tag — `!<tag:yaml.org,2002:int>`, which granit scans to an empty
//! handle with the full URI sitting in `suffix` — is not recognised as
//! core-schema even though `PyYAML` and ruamel.yaml resolve every spelling to the
//! same type (verified). These helpers collapse all three spellings (shorthand
//! `!!int`, a `%TAG`-resolved handle, and verbatim) onto one core-schema suffix,
//! so a rule matches a type regardless of how the author wrote the tag and the
//! verbatim form cannot be used to evade a check.
//!
//! Prefer these over `Tag::is_yaml_core_schema` for any spelling-independent
//! core-schema decision (issue #277).

use granit_parser::Tag;

/// The core-schema tag-handle prefix (`tag:yaml.org,2002:`) that `!!`-shorthand
/// tags resolve to, and that a verbatim core tag carries inside its suffix.
const CORE_SCHEMA_PREFIX: &str = "tag:yaml.org,2002:";

/// The core-schema type suffix this tag resolves to regardless of spelling
/// (`!!int`, a `%TAG`-resolved `tag:yaml.org,2002:` handle, and verbatim
/// `!<tag:yaml.org,2002:int>` all yield `int`), or `None` for any non-core tag.
#[must_use]
pub fn core_schema_suffix(tag: &Tag) -> Option<&str> {
    if tag.handle == CORE_SCHEMA_PREFIX {
        Some(tag.suffix.as_str())
    } else if tag.handle.is_empty() {
        // Verbatim `!<…>` tags carry an empty handle and the full URI in `suffix`.
        tag.suffix.strip_prefix(CORE_SCHEMA_PREFIX)
    } else {
        None
    }
}

/// Whether `tag` resolves to a YAML core-schema type under any spelling.
#[must_use]
pub fn is_core_schema(tag: &Tag) -> bool {
    core_schema_suffix(tag).is_some()
}
