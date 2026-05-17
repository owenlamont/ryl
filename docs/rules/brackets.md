# brackets

## What this rule does

Controls whether YAML flow sequences (`[...]`) are allowed and how much
whitespace they may contain.

## Why this matters

- **Stylistic consistency.** Inline flow sequences read very differently
  from block sequences and projects often want one style throughout.
- **Predictable diffs.** Fixing the spacing rules avoids whitespace-only
  changes when editors normalise.

## Configuration

```toml
[rules.brackets]
level = "error"
forbid = false
min-spaces-inside = 0
max-spaces-inside = 0
min-spaces-inside-empty = -1
max-spaces-inside-empty = -1
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid` | `false` | `false`, `"non-empty"`, or `true`. Forbid flow sequences outright or only when non-empty. |
| `min-spaces-inside` | `0` | Minimum spaces between `[` and the first item (and between the last item and `]`). |
| `max-spaces-inside` | `0` | Maximum spaces at the same positions. |
| `min-spaces-inside-empty` | `-1` | Minimum spaces inside an empty `[]`. `-1` falls back to `min-spaces-inside`. |
| `max-spaces-inside-empty` | `-1` | Maximum spaces inside an empty `[]`. `-1` falls back to `max-spaces-inside`. |

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
object: [1, 2, abc]
```

### :x: Reported (defaults)

```yaml
object: [ 1, 2, abc ]
```

### :x: Reported (with `forbid: true`)

```yaml
object: [1, 2, abc]
```

## Automatic fixing

`ryl --fix` adjusts whitespace inside brackets to satisfy the configured
bounds. The `forbid` constraint is not auto-fixed because converting flow
sequences to block style requires re-flowing the document.

## Related rules

- [`braces`](braces.md) &mdash; the equivalent rule for flow mappings.
- [`commas`](commas.md) &mdash; controls spacing between flow sequence
  items.
- [`hyphens`](hyphens.md) &mdash; spacing rule for block sequences.
