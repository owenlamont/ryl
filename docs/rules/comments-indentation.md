# comments-indentation

## What this rule does

Requires that standalone comment lines line up with the surrounding
content. A comment may share the indentation of the line that follows it or, at a
dedent, retain the preceding block's indentation. At EOF, the preceding line applies.
A line-1 `#!` shebang or `#cloud-config` header is exempt, where yamllint
checks it: moving it would break the file.

## Why this matters

- **Visual grouping.** Comments that float at unrelated indent levels
  confuse readers about which block they describe.
- **Diff stability.** Aligning comments with their content makes it
  obvious when an edit moves or breaks the relationship.

## Configuration

```toml
[lint.rules.comments-indentation]
level = "error"
# Accept a comment aligned to any still-open enclosing block level (default false).
allow-any-open-indent = false
```

### `allow-any-open-indent`

This option is configurable in TOML only; YAML config rejects it. When `true`,
a standalone comment is also accepted if its indentation matches **any
currently-open enclosing block level**, not just the content that
follows it &mdash; useful for a comment that marks where a nested block ends. A
comment indented more deeply than every open level (or at a non-boundary indent) is
still reported. The default is `false`.

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

`ryl check --fix` reindents misaligned standalone comments to match the surrounding
content, leaving accepted comments unchanged. Disable with:

```toml
[lint]
fixable = ["ALL"]
unfixable = ["comments-indentation"]
```

`ryl format` applies the same fix, and no option of this rule conflicts with it.

## Related rules

- [`comments`](comments.md) &mdash; controls the formatting of comment
  text itself.
- [`indentation`](indentation.md) &mdash; the general indentation rule
  these comments are aligned against.
