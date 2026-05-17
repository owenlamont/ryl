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

## Automatic fixing

This rule does not auto-fix; collapse extra empty lines manually.

## Related rules

- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the single trailing newline character (distinct from "empty lines at
  end").
- [`trailing-spaces`](trailing-spaces.md) &mdash; reports whitespace on
  otherwise-empty lines.
