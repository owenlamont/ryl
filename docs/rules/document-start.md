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
[lint.rules.document-start]
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

### :wrench: After `ryl check --fix` (with `present: true`)

```yaml
---
title: example
```

## Automatic fixing

`ryl check --fix` adds a `---` start marker to every document that lacks one when
`present: true`: at the top of the file for the first document, and after the
`...` that ends the previous document for a later one. A UTF-8 BOM, leading the
file or a later document, stays before the `---`. A line-1 `#!` shebang or
cloud-init `#cloud-config` header stays first, with the `---` inserted after it,
since the interpreter or cloud-init reads it from byte 0.

The `present: false` case (removing existing `---` markers) is never auto-fixed
because removal can collide with multi-document boundaries.

Disable with:

```toml
[lint]
fixable = ["ALL"]
unfixable = ["document-start"]
```

`ryl format` adds `---` the same way while `[format] document-start` is `"add"`, the default. It conflicts with `present = false` unless that key is `"preserve"`; see [Conflicting lint rules](../formatter.md#conflicting-lint-rules).

## Related rules

- [`document-end`](document-end.md) &mdash; the matching rule for the
  `...` end marker.
