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
# Accept a comment aligned to any still-open enclosing block level (default false).
allow-any-open-indent = false
```

### `allow-any-open-indent`

A ryl-only option (configurable in TOML only; rejected in yamllint-compatible YAML
config). When `true`, a standalone comment is also accepted if its indentation
matches **any currently-open enclosing block level**, not just the content that
follows it &mdash; useful for a comment that marks where a nested block ends. A
comment indented more deeply than every open level (or at a non-boundary indent) is
still reported. The default `false` keeps the yamllint-compatible behaviour. Origin:
[adrienverge/yamllint#141](https://github.com/adrienverge/yamllint/issues/141).

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

### :white_check_mark: Allowed with `allow-any-open-indent = true`

```yaml
config:
    entry:
        - things
    # aligned to the open `entry:` level — flagged by default, accepted with the option
options:
    - more stuff
```

### :wrench: After `ryl check --fix`

```yaml
parent:
  # this comment is over-indented compared to `child`
  child: value
```

## Automatic fixing

`ryl check --fix` reindents standalone comment lines to match the line that
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
