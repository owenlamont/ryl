# document-end

## What this rule does

Requires or forbids the YAML document end marker (`...`).

## Why this matters

- **Streaming consumers.** Producers that emit multiple YAML documents
  benefit from explicit end markers so consumers know each document is
  complete.
- **Single-document files.** Most single-document YAML files omit `...`;
  forbidding it removes a trailing footer that adds no information.

## Configuration

```toml
[rules.document-end]
level = "error"
present = true
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `present` | `true` | When `true`, require a `...` marker at the end of every document. When `false`, forbid it. |

## Examples

### :white_check_mark: Allowed (with `present: true`)

```yaml
---
this: is the only document
...
```

### :x: Reported (with `present: true`)

```yaml
---
this: is the only document
```

### :white_check_mark: Allowed (with `present: false`)

```yaml
---
this: is the only document
```

## Automatic fixing

This rule does not auto-fix; add or remove the marker manually.

## Related rules

- [`document-start`](document-start.md) &mdash; the matching rule for the
  `---` start marker.
- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the final newline character.
