# new-line-at-end-of-file

## What this rule does

Requires the file to end with a trailing newline character.

## Why this matters

- **POSIX text files** are defined as a sequence of zero or more lines,
  each terminated by a newline. Many tools (concat, cat, version control
  diff renderers) behave unexpectedly without the trailing newline.
- **Editor friendliness.** Most editors silently add or strip a trailing
  newline on save; pinning the rule keeps diffs from oscillating between
  contributors.

## Configuration

```toml
[rules.new-line-at-end-of-file]
level = "error"
```

This rule has no options beyond `level`.

## Examples

### :white_check_mark: Allowed

```yaml
---
key: value
```

(File ends with a single `\n` after the last line.)

### :x: Reported

A file whose final byte is not a newline character. (Trailing *blank* lines
are not this rule's concern; see [`empty-lines`](empty-lines.md).)

### :wrench: After `ryl check --fix`

ryl appends a single newline when the file does not already end with one.

## Automatic fixing

`ryl check --fix` appends a trailing newline when one is missing. Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["new-line-at-end-of-file"]
```

## Related rules

- [`empty-lines`](empty-lines.md) &mdash; controls multiple empty lines
  at the end of file (distinct from the trailing newline).
- [`new-lines`](new-lines.md) &mdash; controls which line ending
  character is used.
