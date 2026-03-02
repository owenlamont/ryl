# ryl config presets (TOML)

These TOML presets mirror the built-in YAML presets in `ryl` (`default`,
`relaxed`, `empty`) from [src/conf/mod.rs](/home/owen/Code/ryl_repos/ryl/src/conf/mod.rs).

## `default` (TOML equivalent)

```toml
yaml-files = ["*.yaml", "*.yml", ".yamllint"]

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
yaml-files = ["*.yaml", "*.yml", ".yamllint"]

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

## `empty` (TOML equivalent)

```toml
[rules]
```
