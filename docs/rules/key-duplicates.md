# key-duplicates

## What this rule does

Reports duplicate keys in a single mapping. Optionally also reports
duplicated `<<` merge keys.

## Why this matters

- **Silent overwrites.** When two entries share a key, most YAML parsers
  keep only one of them; the other's value is silently discarded.
- **Schema validation.** Some downstream consumers reject documents with
  duplicate keys outright, so catching the duplication early avoids
  surprises in production.

## Configuration

```toml
[rules.key-duplicates]
level = "error"
forbid-duplicated-merge-keys = false
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid-duplicated-merge-keys` | `false` | Also report duplicate `<<` merge keys in the same mapping. |

## Examples

### :white_check_mark: Allowed

```yaml
---
first: 1
second: 2
```

### :x: Reported

```yaml
---
key: 1
key: 2
```

### :x: Reported (with `forbid-duplicated-merge-keys: true`)

```yaml
---
<<: *anchor-one
<<: *anchor-two
extra: value
```

## Automatic fixing

This rule does not auto-fix; resolving a duplicate requires deciding
which value is canonical.

## Related rules

- [`key-ordering`](key-ordering.md) &mdash; enforces alphabetical key
  order, which makes duplicates harder to introduce accidentally.
- [`anchors`](anchors.md) &mdash; covers a related class of ambiguity
  for anchor/alias declarations.
