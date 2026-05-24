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

### :wrench: After `ryl --fix`

```yaml
---
key: value
```

## Automatic fixing

`ryl --fix` strips trailing spaces and tabs from each line. The fix is
**partial** by design: lines inside literal/folded block scalars
(`|`/`>`) and inside multi-line double-quoted scalars are left untouched,
because in those contexts trailing whitespace can be part of the parsed
scalar value. The diagnostic still fires on those lines so the
remaining trailing whitespace is visible after `--fix`; edit them by
hand if you want them clean. Multi-line single-quoted and plain scalars
fold trailing whitespace away at parse time, so the fix can safely
strip those.

The fix bails (leaves the file untouched) when the input cannot be
parsed as YAML, so a broken document is never made worse.

Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["trailing-spaces"]
```

## Related rules

- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the trailing newline character.
- [`empty-lines`](empty-lines.md) &mdash; controls fully blank lines (a
  separate concern from trailing whitespace on content lines).
