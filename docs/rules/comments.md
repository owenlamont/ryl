# comments

## What this rule does

Controls formatting of `#` comments &mdash; whether a space is required
after the `#`, and how far inline comments must sit from preceding
content.

## Why this matters

- **Legibility.** `#comment` and `# comment` read very differently;
  enforcing a space keeps comments visually distinct from directive-like
  prefixes.
- **Inline comments.** Pushing inline comments away from values prevents
  visual collisions when values change length; capping the gap keeps them
  next to the value they describe.

## Configuration

```toml
[rules.comments]
level = "error"
require-starting-space = true
ignore-shebangs = true
min-spaces-from-content = 2
max-spaces-from-content = -1
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `require-starting-space` | `true` | Require at least one space between `#` and the comment text. |
| `ignore-shebangs` | `true` | Skip `#!` shebang lines when `require-starting-space` is on. |
| `min-spaces-from-content` | `2` | Minimum spaces between code and an inline `#` comment. Use `-1` to disable. |
| `max-spaces-from-content` | `-1` | Maximum spaces between code and an inline `#` comment; `-1` (the default) disables it. Must be at least 1 and at least `min-spaces-from-content`. TOML only. |

Spaces and tabs each count as one. Setting `max-spaces-from-content` reports
deliberately column-aligned inline comments, and `--fix` collapses the alignment.

A comment after a block scalar header (`key: >- # note`) is checked like any other
inline comment, where yamllint skips it; see
[How ryl differs from yamllint](../getting-started/migrating-from-yamllint.md#comments-after-a-block-scalar-header).

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
# a properly spaced comment
key: value  # inline comment with two spaces of padding
```

### :x: Reported (defaults)

```yaml
#missing space after the hash
key: value # only one space before inline comment
```

### :wrench: After `ryl check --fix`

```yaml
# missing space after the hash
key: value  # only one space before inline comment
```

### :x: Reported (`min-spaces-from-content = 2`, `max-spaces-from-content = 2`)

```yaml
first: value        # too far from its value
second: value # too close
```

### :wrench: After `ryl check --fix`

```yaml
first: value  # too far from its value
second: value  # too close
```

## Automatic fixing

`ryl check --fix` inserts the missing space after `#`, pads inline comments
to the configured `min-spaces-from-content`, and replaces a gap wider than
`max-spaces-from-content` with that many spaces. Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["comments"]
```

## Related rules

- [`comments-indentation`](comments-indentation.md) &mdash; controls the
  vertical alignment of standalone comments.
