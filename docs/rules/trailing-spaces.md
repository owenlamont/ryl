# trailing-spaces

## What this rule does

Reports whitespace at the end of any line.

## Why this matters

- **Invisible drift.** Trailing whitespace is invisible in most editors
  but shows up as a change in every diff, polluting code review.
- **Editor normalisation.** Many editors strip trailing whitespace on
  save; without this rule, contributors with different editor settings
  produce conflicting commits.

## Configuration

```toml
[rules.trailing-spaces]
level = "error"
```

This rule has no options beyond `level`.

## Examples

### :white_check_mark: Allowed

```yaml
---
key: value
```

(No spaces between `value` and the newline.)

### :x: Reported

```yaml
---
key: value···
```

(Where `···` represents trailing whitespace characters.)

## Automatic fixing

This rule does not auto-fix in ryl. Most editors and pre-commit hooks
strip trailing whitespace; rely on those when available.

## Related rules

- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the trailing newline character.
- [`empty-lines`](empty-lines.md) &mdash; controls fully blank lines (a
  separate concern from trailing whitespace on content lines).
