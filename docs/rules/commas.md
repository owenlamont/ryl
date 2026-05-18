# commas

## What this rule does

Controls the number of spaces before and after commas (`,`) in flow
mappings and flow sequences.

## Why this matters

- **Readability.** `[1, 2, 3]` is easier to scan than `[1,2 ,  3]`.
- **Consistent diffs.** Normalising comma spacing prevents whitespace-only
  diffs.

## Configuration

```toml
[rules.commas]
level = "error"
max-spaces-before = 0
min-spaces-after = 1
max-spaces-after = 1
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `max-spaces-before` | `0` | Maximum spaces before the comma. Use `-1` to disable. |
| `min-spaces-after` | `1` | Minimum spaces after the comma. |
| `max-spaces-after` | `1` | Maximum spaces after the comma. Use `-1` to disable. |

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
list: [10, 20, 30, {x: 1, y: 2}]
```

### :x: Reported (defaults)

```yaml
list: [10, 20 ,30,   {x: 1,   y: 2}]
```

### :wrench: After `ryl --fix`

```yaml
list: [10, 20, 30, {x: 1, y: 2}]
```

## Automatic fixing

`ryl --fix` normalises whitespace around commas to satisfy the configured
limits. Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["commas"]
```

## Related rules

- [`braces`](braces.md) and [`brackets`](brackets.md) &mdash; spacing
  inside the flow delimiters themselves.
- [`colons`](colons.md) &mdash; the analogous rule for mapping colons.
