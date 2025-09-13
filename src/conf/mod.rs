#![allow(clippy::module_name_repetitions)]

// Built-in presets to support `extends`, mirroring yamllint.

#[must_use]
pub fn builtin(name: &str) -> Option<&'static str> {
    match name {
        "default" => Some(DEFAULT),
        "relaxed" => Some(RELAXED),
        "empty" => Some(EMPTY),
        _ => None,
    }
}

const DEFAULT: &str = r"---

yaml-files:
  - '*.yaml'
  - '*.yml'
  - '.yamllint'

rules:
  anchors: enable
  braces: enable
  brackets: enable
  colons: enable
  commas: enable
  comments:
    level: warning
  comments-indentation:
    level: warning
  document-end: disable
  document-start:
    level: warning
  empty-lines: enable
  empty-values: disable
  float-values: disable
  hyphens: enable
  indentation: enable
  key-duplicates: enable
  key-ordering: disable
  line-length: enable
  new-line-at-end-of-file: enable
  new-lines: enable
  octal-values: disable
  quoted-strings: disable
  trailing-spaces: enable
  truthy:
    level: warning
";

const RELAXED: &str = r"---

extends: default

rules:
  braces:
    level: warning
    max-spaces-inside: 1
  brackets:
    level: warning
    max-spaces-inside: 1
  colons:
    level: warning
  commas:
    level: warning
  comments: disable
  comments-indentation: disable
  document-start: disable
  empty-lines:
    level: warning
  hyphens:
    level: warning
  indentation:
    level: warning
    indent-sequences: consistent
  line-length:
    level: warning
    allow-non-breakable-inline-mappings: true
  truthy: disable
";

const EMPTY: &str = r"
rules: {}
";
