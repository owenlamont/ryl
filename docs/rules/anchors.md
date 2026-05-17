# anchors

## What this rule does

Reports problems with YAML anchors and aliases &mdash; duplicated anchor
names, aliases that reference undeclared anchors, and (optionally) anchors
that are declared but never used.

## Why this matters

- **Forward references fail at runtime.** An alias that points at an
  anchor declared later in the document is rejected by most YAML parsers.
- **Duplicates are silently ambiguous.** When two `&name` declarations
  share a name, downstream consumers may bind to either one depending on
  parser order.
- **Unused anchors clutter intent.** Removing dead anchors keeps the
  document's data-sharing structure explicit.

## Configuration

```toml
[rules.anchors]
level = "error"
forbid-undeclared-aliases = true
forbid-duplicated-anchors = false
forbid-unused-anchors = false
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid-undeclared-aliases` | `true` | Report aliases (`*name`) whose anchor (`&name`) has not been declared earlier. |
| `forbid-duplicated-anchors` | `false` | Report when the same anchor name is declared more than once. |
| `forbid-unused-anchors` | `false` | Report anchors that are never referenced by an alias. |

## Examples

### :white_check_mark: Allowed

```yaml
---
- &anchor
  foo: bar
- *anchor
```

### :x: Reported (with `forbid-undeclared-aliases: true`)

```yaml
---
- &anchor
  foo: bar
- *unknown
```

### :x: Reported (with `forbid-duplicated-anchors: true`)

```yaml
---
- &anchor Foo Bar
- &anchor [item 1, item 2]
```

## Automatic fixing

This rule does not auto-fix; anchor and alias graphs require human
judgement to rewrite safely.

## Related rules

- [`key-duplicates`](key-duplicates.md) &mdash; covers a related class of
  ambiguity in mappings.
