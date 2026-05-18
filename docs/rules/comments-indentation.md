# comments-indentation

## What this rule does

Requires that standalone comment lines line up with the surrounding
content. A comment must share the indentation of the line that follows it
(or, when at the end of the file, the line that precedes it).

## Why this matters

- **Visual grouping.** Comments that float at unrelated indent levels
  confuse readers about which block they describe.
- **Diff stability.** Aligning comments with their content makes it
  obvious when an edit moves or breaks the relationship.

## Configuration

```toml
[rules.comments-indentation]
level = "error"
```

This rule has no options beyond `level`.

## Examples

### :white_check_mark: Allowed

```yaml
parent:
  # describes the child key
  child: value
```

### :x: Reported

```yaml
parent:
    # this comment is over-indented compared to `child`
  child: value
```

### :wrench: After `ryl --fix`

```yaml
parent:
  # this comment is over-indented compared to `child`
  child: value
```

## Automatic fixing

`ryl --fix` reindents standalone comment lines to match the line that
follows them. Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["comments-indentation"]
```

## Related rules

- [`comments`](comments.md) &mdash; controls the formatting of comment
  text itself.
- [`indentation`](indentation.md) &mdash; the general indentation rule
  these comments are aligned against.
