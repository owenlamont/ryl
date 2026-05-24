# document-start

## What this rule does

Requires or forbids the YAML document start marker (`---`).

## Why this matters

- **Multi-document streams.** Files containing more than one YAML
  document need `---` to separate them.
- **Explicit intent.** Requiring `---` even in single-document files
  makes the format unambiguous and clearly signals that the file is
  YAML rather than another similar markup.

## Configuration

```toml
[rules.document-start]
level = "error"
present = true
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `present` | `true` | When `true`, require a `---` marker at the start of every document. When `false`, forbid it. |

## Examples

### :white_check_mark: Allowed (with `present: true`)

```yaml
---
title: example
```

### :x: Reported (with `present: true`)

```yaml
title: example
```

### :white_check_mark: Allowed (with `present: false`)

```yaml
title: example
```

### :wrench: After `ryl --fix` (with `present: true`)

```yaml
---
title: example
```

## Automatic fixing

`ryl --fix` prepends a `---` start marker when `present: true` and the
document does not already have one. The fix is **partial** by design:
it only runs when

- the buffer is a single document (no inner `---`/`...` markers anywhere
  in the file),
- the buffer does not begin (after leading comments and blank lines)
  with a `%YAML`/`%TAG` directive, and
- a leading UTF-8 BOM, if any, stays at byte 0 with the new `---`
  inserted after it.

Multi-document streams and files with directives are left for manual
intervention, because the correct marker placement there depends on
context the rule does not record. The `present: false` case (removing
existing `---` markers) is never auto-fixed because removal can collide
with multi-document boundaries.

Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["document-start"]
```

## Related rules

- [`document-end`](document-end.md) &mdash; the matching rule for the
  `...` end marker.
