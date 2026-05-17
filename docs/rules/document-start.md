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

## Automatic fixing

This rule does not auto-fix; add or remove the marker manually.

## Related rules

- [`document-end`](document-end.md) &mdash; the matching rule for the
  `...` end marker.
