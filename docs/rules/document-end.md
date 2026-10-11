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
[lint.rules.document-end]
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

### :wrench: After `ryl check --fix` (with `present: true`)

```yaml
---
this: is the only document
...
```

## Automatic fixing

`ryl check --fix` adds a `...` end marker to every document that lacks one
when `present: true`: before the `---` that opens the next document, and at
the end of the stream. The `present: false` case (removing existing `...`
markers) is never auto-fixed. A block scalar header with no body at EOF can gain the
newline and marker; an unterminated clip/keep body is left alone to preserve its value.

Disable with:

```toml
[lint]
fixable = ["ALL"]
unfixable = ["document-end"]
```

`ryl format` adds `...` the same way only when `[format] document-end` is `"add"`, and then conflicts with `present = false`; see [Conflicting lint rules](../formatter.md#conflicting-lint-rules).

## Related rules

- [`document-start`](document-start.md) &mdash; the matching rule for the
  `---` start marker.
- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the final newline character.
