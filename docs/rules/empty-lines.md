# empty-lines

## What this rule does

Controls the number of consecutive empty lines allowed in the body of the
file, at the start of the file, and at the end of the file.

## Why this matters

- **Vertical density.** Too many blank lines fragment related content;
  too few make sections run together.
- **Trailing blanks.** Empty lines at the end of a file are usually
  unintentional and produce noisy diffs.

## Configuration

```toml
[rules.empty-lines]
level = "error"
max = 2
max-start = 0
max-end = 0
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `max` | `2` | Maximum consecutive empty lines anywhere in the document. |
| `max-start` | `0` | Maximum empty lines allowed at the start of the file. |
| `max-end` | `0` | Maximum empty lines allowed at the end of the file. |

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
---
a: 1

b: 2


c: 3
```

### :x: Reported (defaults)

```yaml
---
a: 1



b: 2
```

### :wrench: After `ryl --fix` (defaults)

```yaml
---
a: 1


b: 2
```

## Automatic fixing

`ryl --fix` trims runs of empty lines down to `max` (or `max-start`
and `max-end` for the leading and trailing run). The fix is **partial**
by design: blank lines that fall inside any multi-line scalar — literal
or folded block scalars (`|`/`>`), multi-line single- or double-quoted
scalars, or multi-line plain scalars — are left untouched, because
those blank lines contribute to the parsed value.

The protected line set is computed via the YAML parser, so the fix
bails (leaves the file untouched) when the input cannot be parsed.

Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["empty-lines"]
```

## Related rules

- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the single trailing newline character (distinct from "empty lines at
  end").
- [`trailing-spaces`](trailing-spaces.md) &mdash; reports whitespace on
  otherwise-empty lines.
