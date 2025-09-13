#![allow(clippy::module_name_repetitions)]

// Minimal built-in presets to support `extends`.
// These are placeholders to enable composition and merging logic.

#[must_use]
pub fn builtin(name: &str) -> Option<&'static str> {
    match name {
        "default" => Some(DEFAULT),
        "relaxed" => Some(RELAXED),
        "empty" => Some(EMPTY),
        _ => None,
    }
}

const DEFAULT: &str = r"
rules:
  trailing-spaces: enable
  document-end: enable
";

const RELAXED: &str = r"
rules:
  trailing-spaces: disable
";

const EMPTY: &str = r"
rules: {}
";
