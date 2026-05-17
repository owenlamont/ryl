# new-lines

## What this rule does

Enforces a consistent line-ending style across the file &mdash; either
Unix (LF), DOS/Windows (CRLF), or whatever the host platform produces.

## Why this matters

- **Cross-platform contributors.** Mixed CRLF and LF line endings show up
  in diffs as full-file changes when a contributor's editor normalises
  them.
- **Tool compatibility.** Some YAML consumers and shell utilities behave
  unexpectedly with the "wrong" line ending for the platform.

## Configuration

```toml
[rules.new-lines]
level = "error"
type = "unix"
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `type` | `"unix"` | `"unix"` (LF), `"dos"` (CRLF), or `"platform"` to match the host operating system. |

## Examples

The rule operates on the bytes of the file, so visible examples are
limited. With `type: "unix"`:

### :white_check_mark: Allowed

A file whose every line ends with `\n`.

### :x: Reported

A file whose lines end with `\r\n`.

### :wrench: After `ryl --fix`

`ryl --fix` rewrites the file so every line ends with the configured
character sequence.

## Automatic fixing

`ryl --fix` rewrites all line endings to match `type`. Disable with:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["new-lines"]
```

## Related rules

- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the single trailing newline at end of file.
- [`trailing-spaces`](trailing-spaces.md) &mdash; reports whitespace
  before the line ending.
