# indentation

## What this rule does

Enforces consistent indentation throughout the document. Specifically:

- The indent width used by block mappings and block sequences.
- Whether block sequence items are indented under their parent key or
  start at the same column.
- Optionally, the indentation of content inside multi-line block scalars.

## Why this matters

- **Mixed indents cause parse surprises.** YAML's whitespace sensitivity
  means inconsistent indents can change which key a value belongs to.
- **Cross-tool diffing.** Editors with different default tab widths can
  silently introduce mixed indentation; the rule catches drift early.
- **Convention.** Most projects settle on either 2- or 4-space indents;
  pinning the width prevents new files from drifting.

## Configuration

```toml
[rules.indentation]
level = "error"
spaces = "consistent"
indent-sequences = true
check-multi-line-strings = false
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `spaces` | `"consistent"` | An integer such as `2` for a fixed indent width, or `"consistent"` to lock the rest of the file to the first indent seen. |
| `indent-sequences` | `true` | `true`, `false`, `"whatever"`, or `"consistent"`. Whether block sequence items are indented under the parent key. |
| `check-multi-line-strings` | `false` | When `true`, apply indent checks inside block scalars and multi-line flow strings. |

## Examples

### :white_check_mark: Allowed (with `spaces: 2, indent-sequences: true`)

```yaml
parent:
  list:
    - item one
    - item two
```

### :x: Reported (with `spaces: 2, indent-sequences: true`)

```yaml
parent:
  list:
  - item one
  - item two
```

### :white_check_mark: Allowed (with `indent-sequences: false`)

```yaml
parent:
  list:
  - item one
  - item two
```

### :x: Reported (with `spaces: 2`)

```yaml
parent:
   over-indented: value
```

## Automatic fixing

This rule does not auto-fix; indentation changes can shift values into
different parents, so corrections need human review.

## Related rules

- [`hyphens`](hyphens.md) &mdash; spacing between `-` and the sequence
  item value.
- [`comments-indentation`](comments-indentation.md) &mdash; standalone
  comments must follow the same indent as the next line.
