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

### :wrench: After `ryl check --fix` (with `present: true`)

```yaml
---
this: is the only document
...
```

## Automatic fixing

`ryl check --fix` appends a `...` end marker when `present: true` and the
document does not already have one. The fix is **partial** by design:
it only runs when the buffer is a single document. A buffer is treated
as single-document when, after skipping leading blank lines, comments,
and `%`-directive lines:

- it contains at most one `---` marker, and that marker is not preceded
  by real (non-comment, non-directive) content, and
- it contains no `...` marker anywhere.

Multi-document streams are left for manual intervention because each
document needs its own `...` placed at the correct byte offset, and the
rule does not record per-document end positions. The `present: false`
case (removing existing `...` markers) is never auto-fixed.

Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["document-end"]
```

## Related rules

- [`document-start`](document-start.md) &mdash; the matching rule for the
  `---` start marker.
- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the final newline character.
