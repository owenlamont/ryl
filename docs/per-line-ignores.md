# Per-line ignores

## Suppressing rules from configuration

[Inline directives](directives.md) switch rules off at a specific spot in a
file. `per-line-ignores` does the same thing from configuration: it suppresses
chosen rules on every line matching a pattern, across all files, without editing
them. It is the tool for recurring exceptions &mdash; tool-directive comments and
machine-managed markers that legitimately break a rule everywhere they appear.

Two common cases:

- `#cloud-config` directives must keep their exact spelling, so they trip
  [`comments`](rules/comments.md) (`require-starting-space`).
- `# renovate:` markers can be long and cannot be wrapped, so they trip
  [`line-length`](rules/line-length.md).

`per-line-ignores` is **ryl-only** and configured in TOML only (yamllint has no
equivalent); it is rejected in yamllint-compatible YAML config.

## Configuration

Each `[[per-line-ignores]]` entry suppresses its `rules` on a line when the
entry's conditions match:

```toml
[rules.comments]
[rules.line-length]
max = 80

[[per-line-ignores]]
regex = '^#cloud-config$'   # match against the whole source line
rules = ["comments"]

[[per-line-ignores]]
regex = '#\s*renovate:'
rules = ["line-length"]
```

| Field | Required | Description |
| :--- | :--- | :--- |
| `regex` | one of `regex`/`path` | Regex matched against the whole physical source line (unanchored &mdash; add `^`/`$` yourself). |
| `path` | one of `regex`/`path` | Glob matched against the file path (same glob semantics as [`per-file-ignores`](config-presets.md), including a leading `!` to negate &mdash; apply to files *not* matching). |
| `rules` | yes | Rule IDs to suppress, or `["ALL"]` for every rule. |

Present conditions are combined with logical **AND**: an entry with both `regex` and
`path` suppresses its rules only on matching lines of matching files. An omitted field is
unconstrained &mdash; no `path` means every file; no `regex` means every line of the
matched files. At least one of `regex`/`path` is required, so an entry can never
disable a rule globally (use the rule's own config to turn it off).

Use single-quoted TOML strings for patterns so backslashes need no escaping.

### Examples

```toml
# Let machine-generated marker lines break any rule
[[per-line-ignores]]
regex = 'GENERATED — do not edit'
rules = ["ALL"]

# Allow Go-template braces, but only in template files
[[per-line-ignores]]
path = "*.tpl.yaml"
regex = '\{\{.*\}\}'
rules = ["braces"]
```

## Behaviour

- **Matching** is unanchored, so `^#cloud-config` only matches a comment at the
  start of a line, while `renovate:` matches anywhere on it.
- **`--fix`** never rewrites a suppressed line: an edit a fixer would make to a
  line whose rule is suppressed here is reverted, exactly as for an inline
  `# ryl disable-line`.
- **Embedded [YAML in Markdown](markdown.md)**: `path` matches the host Markdown
  file, while `regex` matches the embedded YAML line (after any list/blockquote
  prefix is stripped), so the same pattern works for standalone and embedded YAML.
- **Syntax errors are never suppressed** &mdash; `per-line-ignores` only mutes the
  named rules, never a parse failure.

## Related

- [Inline directives](directives.md) &mdash; per-file, in-line suppression with
  `# ryl disable` / `disable-line`.
