# colons

## What this rule does

Controls the number of spaces around mapping colons (`:`).

## Why this matters

- **Readability.** Consistent spacing makes columnar layouts easier to
  scan, especially in configuration files with many short keys.
- **Avoids ambiguity.** Stray spaces around the colon can hide subtle
  parsing bugs, particularly when keys contain values that look like
  flow-style content.

## Configuration

```toml
[rules.colons]
level = "error"
max-spaces-before = 0
max-spaces-after = 1
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `max-spaces-before` | `0` | Maximum spaces between the key and the `:`. Use `-1` to disable. |
| `max-spaces-after` | `1` | Maximum spaces between the `:` and the value. Use `-1` to disable. |

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
key: value
object:
  - a
  - b
```

### :x: Reported (defaults)

```yaml
key : value
key:   value
```

### :white_check_mark: Allowed (with `max-spaces-after: 2`)

```yaml
first:  1
second: 2
third:  3
```

### Alias mapping keys

A YAML anchor/alias name may legally contain `:`, so `*anchor:` is read as the alias to
an anchor named `anchor:`. Using an alias as a mapping key therefore *requires* one space
before the colon &mdash; `*anchor : value`. When that required space is present the
colon's spacing is not reported (matching the parser's view of the alias); any other
spacing is checked as usual.

```yaml
anchor: &a 42
*a : value     # allowed: the required space is not reported
*a  : value    # reported: an extra space before the colon
*a:  value     # reported: too many spaces after the colon
```

## Automatic fixing

This rule does not auto-fix; correct spacing manually.

## Related rules

- [`commas`](commas.md) &mdash; the analogous rule for flow collection
  commas.
- [`hyphens`](hyphens.md) &mdash; spacing for block sequence hyphens.
