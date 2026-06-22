# braces

## What this rule does

Controls whether YAML flow mappings (`{...}`) are allowed and how much
whitespace they may contain.

## Why this matters

- **Stylistic consistency.** Mixing `{a: 1, b: 2}` and block mappings in
  the same file makes diffs noisier and intent harder to scan.
- **Tooling friendliness.** Some YAML consumers handle block style better
  than flow style; forbidding flow mappings can simplify downstream
  parsing.

## Configuration

```toml
[rules.braces]
level = "error"
forbid = false
min-spaces-inside = 0
max-spaces-inside = 0
min-spaces-inside-empty = -1
max-spaces-inside-empty = -1
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid` | `false` | `false`, `"non-empty"`, or `true`. Forbid flow mappings outright or only when non-empty. |
| `min-spaces-inside` | `0` | Minimum spaces between `{` and the first key (and between the last value and `}`). |
| `max-spaces-inside` | `0` | Maximum spaces at the same positions. |
| `min-spaces-inside-empty` | `-1` | Minimum spaces inside an empty `{}`. `-1` falls back to `min-spaces-inside`. |
| `max-spaces-inside-empty` | `-1` | Maximum spaces inside an empty `{}`. `-1` falls back to `max-spaces-inside`. |

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
object: {key1: 4, key2: 8}
```

### :x: Reported (defaults)

```yaml
object: { key1: 4, key2: 8 }
```

### :x: Reported (with `forbid: non-empty`)

```yaml
object: {key1: 4, key2: 8}
```

## Automatic fixing

`ryl check --fix` adjusts whitespace inside braces to satisfy the configured
`min-spaces-inside` / `max-spaces-inside` bounds. The `forbid` constraint
is not auto-fixed because converting flow to block style requires
re-flowing the surrounding document.

## Related rules

- [`brackets`](brackets.md) &mdash; the equivalent rule for flow
  sequences `[...]`.
- [`commas`](commas.md) &mdash; controls spacing between flow mapping
  entries.
