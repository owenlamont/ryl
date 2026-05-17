# key-ordering

## What this rule does

Requires that the keys within each mapping appear in lexicographic
(locale-aware) order.

## Why this matters

- **Predictable diffs.** When new keys are inserted in sorted order,
  diffs are localised to the area of change.
- **Reviewability.** A consistent key order makes it easy to spot when a
  key is missing or misnamed.

## Configuration

```toml
[rules.key-ordering]
level = "error"
ignored-keys = []
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `ignored-keys` | `[]` | Regular expressions; keys matching any pattern may appear in any order. |

## Examples

### :white_check_mark: Allowed

```yaml
---
alpha: 1
beta: 2
gamma: 3
```

### :x: Reported

```yaml
---
gamma: 3
alpha: 1
beta: 2
```

### :white_check_mark: Allowed (with `ignored-keys: ["^x-"]`)

```yaml
---
x-trailing-extension: value
alpha: 1
beta: 2
```

## Automatic fixing

This rule does not auto-fix; reordering keys can disturb mappings whose
order is significant to readers (for example documenting fields in a
logical workflow).

## Related rules

- [`key-duplicates`](key-duplicates.md) &mdash; ordering and uniqueness
  are commonly enforced together.
