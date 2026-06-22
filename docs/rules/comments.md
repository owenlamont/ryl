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
  visual collisions when values change length.

## Configuration

```toml
[rules.comments]
level = "error"
require-starting-space = true
ignore-shebangs = true
min-spaces-from-content = 2
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `require-starting-space` | `true` | Require at least one space between `#` and the comment text. |
| `ignore-shebangs` | `true` | Skip `#!` shebang lines when `require-starting-space` is on. |
| `min-spaces-from-content` | `2` | Minimum spaces between code and an inline `#` comment. Use `-1` to disable. |

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

## Automatic fixing

`ryl check --fix` inserts the missing space after `#` and pads inline comments
to the configured `min-spaces-from-content`. Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["comments"]
```

## Related rules

- [`comments-indentation`](comments-indentation.md) &mdash; controls the
  vertical alignment of standalone comments.
