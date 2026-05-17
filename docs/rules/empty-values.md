# empty-values

## What this rule does

Reports keys whose value is missing (empty), separately for block
mappings, flow mappings, and block sequences.

## Why this matters

- **Accidental nulls.** A key without a value parses as `null`, which is
  often unintentional and can mask typos in the value.
- **Schema validation.** Some downstream consumers treat missing values
  differently from explicit `null`, leading to surprising bugs.

## Configuration

```toml
[rules.empty-values]
level = "error"
forbid-in-block-mappings = true
forbid-in-flow-mappings = true
forbid-in-block-sequences = true
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid-in-block-mappings` | `true` | Forbid empty values in block-style mappings. |
| `forbid-in-flow-mappings` | `true` | Forbid empty values in flow-style mappings. |
| `forbid-in-block-sequences` | `true` | Forbid empty list items in block-style sequences. |

## Examples

### :white_check_mark: Allowed

```yaml
---
some-key: value
explicit-null: null
```

### :x: Reported (with `forbid-in-block-mappings: true`)

```yaml
---
some-key:
other-key: value
```

### :x: Reported (with `forbid-in-flow-mappings: true`)

```yaml
---
inline: {first: 1, second: }
```

### :x: Reported (with `forbid-in-block-sequences: true`)

```yaml
---
items:
  -
  - value
```

## Automatic fixing

This rule does not auto-fix; supplying a value (including an explicit
`null`) is a content change that requires intent from the author.

## Related rules

- [`truthy`](truthy.md) &mdash; complementary rule for ambiguous bareword
  values.
- [`quoted-strings`](quoted-strings.md) &mdash; controls quoting of
  values, including the literal string `"null"`.
