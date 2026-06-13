# ryl config presets

## Applying a preset

ryl applies one configuration file to each file it lints. The nearest
config in that file's directory or a parent wins, and any configs further
up are ignored; ryl never merges several together. A monorepo can hold
many `ryl.toml` files (one per subtree), each governed by exactly one of
them. Every rule is enabled explicitly, so the config that applies fully
describes what is checked, with no default-on rules.

Presets are starting points for a config, not something ryl inherits behind
the scenes:

- **TOML config (recommended):** there is no `preset` or `extends` key. The
  tables below *are* the presets, so copy the one you want into your
  `.ryl.toml` (or `ryl.toml`) and customise from there. That copy is then your
  one explicit config.
- **YAML config (yamllint parity):** a yamllint-style config may use
  `extends: default`, `extends: relaxed`, or `extends: empty` to start from a
  preset and override individual rules under `rules:`. This exists for
  compatibility with yamllint; the TOML format has no equivalent by design.

These TOML presets mirror the built-in YAML presets in `ryl` (`default`,
`relaxed`, `empty`) from
[src/conf/mod.rs](https://github.com/owenlamont/ryl/blob/main/src/conf/mod.rs).

## `default` (TOML equivalent)

```toml
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[rules]
anchors = "enable"
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
document-end = "disable"
empty-lines = "enable"
empty-values = "disable"
float-values = "disable"
hyphens = "enable"
indentation = "enable"
key-duplicates = "enable"
key-ordering = "disable"
line-length = "enable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"

[rules.comments]
level = "warning"

[rules.comments-indentation]
level = "warning"

[rules.document-start]
level = "warning"

[rules.truthy]
level = "warning"
```

## `relaxed` (TOML equivalent, fully expanded)

```toml
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

[rules]
anchors = "enable"
comments = "disable"
comments-indentation = "disable"
document-end = "disable"
document-start = "disable"
empty-values = "disable"
float-values = "disable"
key-duplicates = "enable"
key-ordering = "disable"
new-line-at-end-of-file = "enable"
new-lines = "enable"
octal-values = "disable"
quoted-strings = "disable"
trailing-spaces = "enable"
truthy = "disable"

[rules.braces]
level = "warning"
max-spaces-inside = 1

[rules.brackets]
level = "warning"
max-spaces-inside = 1

[rules.colons]
level = "warning"

[rules.commas]
level = "warning"

[rules.empty-lines]
level = "warning"

[rules.hyphens]
level = "warning"

[rules.indentation]
level = "warning"
indent-sequences = "consistent"

[rules.line-length]
level = "warning"
allow-non-breakable-inline-mappings = true
```

## `empty`

There is no usable TOML equivalent: ryl requires at least one rule to be
enabled, so an empty `[rules]` table is rejected with "configuration enables no
rules". The `empty` preset survives only as a base to `extends:` in YAML config;
in TOML, list the rules you want under `[rules]` directly.
