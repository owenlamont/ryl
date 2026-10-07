# line-length

## What this rule does

Enforces a maximum visible width for each line in a YAML file. Lines that
exceed `max` characters are reported.

## Why this matters

- **Reviewability.** Long lines wrap awkwardly in code review tools, diffs,
  and terminal viewers.
- **Side-by-side editing.** Many editors are configured for two-pane work
  at 80 or 100 columns; consistent line lengths keep both panes legible.
- **Generated YAML.** Linting line length is a low-cost way to catch
  templating bugs that produce runaway concatenated values.

## Examples

### :white_check_mark: Allowed (at the default `max = 80`)

```yaml
description: A short summary that fits within the configured line limit.
```

### :x: Reported

```yaml
description: This single line is well over eighty characters wide and so the line-length rule will flag it as too long.
```

### :wrench: How to fix

Break the value onto multiple lines using a block scalar:

```yaml
description: >
  This text was previously one very long line, but is now folded onto
  multiple shorter lines that all fit within the configured width.
```

## Configuration

```toml
[lint.rules.line-length]
level = "warning"
max = 80
allow-non-breakable-words = true
allow-non-breakable-inline-mappings = false
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `max` | top-level `line-length` if set, else `80` | Maximum number of characters allowed per line. |
| `allow-non-breakable-words` | `true` | Allow over-length lines whose long token has no whitespace to break on (typical for URLs or hashes). |
| `allow-non-breakable-inline-mappings` | `false` | Extend the above allowance to lines like `key: <single long token>` where the value has no break candidate. |

## Automatic fixing

`ryl check --fix` does not fix this rule; long lines need to be reflowed by
hand or wrapped with a block scalar.

With `[format] fold-long-lines = true`, `ryl format` folds long plain scalars to the
top-level `line-length`; see [Long lines](../formatter.md#long-lines). No option of this
rule conflicts with it.

## Related rules

- [`empty-lines`](empty-lines.md) &mdash; controls vertical density which
  often pairs with horizontal width preferences.
- [`indentation`](indentation.md) &mdash; deep indentation makes the
  effective content width narrower; consider both rules together when
  picking a `max`.
