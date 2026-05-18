# hyphens

## What this rule does

Controls the number of spaces between the hyphen (`-`) and the value in
block sequences.

## Why this matters

- **Alignment.** Consistent spacing keeps sequence items lined up under
  the same column, which matters for readability of long lists.
- **Diff stability.** Editors that "fix" hyphen spacing during a save
  produce noisy diffs when the rule is unset.

## Configuration

```toml
[rules.hyphens]
level = "error"
max-spaces-after = 1
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `max-spaces-after` | `1` | Maximum spaces between the `-` and the item value. |

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
list:
  - first
  - second
```

### :x: Reported (defaults)

```yaml
list:
  -  first
  -   second
```

### :white_check_mark: Allowed (with `max-spaces-after: 3`)

```yaml
list:
  -   first
  -   second
```

## Automatic fixing

This rule does not auto-fix; trim the extra spaces manually.

## Related rules

- [`indentation`](indentation.md) &mdash; controls how sequence items are
  indented relative to their parent key.
- [`brackets`](brackets.md) and [`commas`](commas.md) &mdash; the flow
  sequence equivalents.
