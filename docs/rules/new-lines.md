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
[lint.rules.new-lines]
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

### :wrench: After `ryl check --fix`

`ryl check --fix` rewrites the file so every line ends with the configured
character sequence.

## Bare carriage returns

A bare `\r` (a carriage return not part of `\r\n`) is a YAML 1.2 line break, so
ryl treats it as a line ending here too. It is never one of the configurable
styles (`unix`/`dos`/`platform`), so when the file's first line break is a bare
`\r` the rule reports it as wrong and `ryl check --fix` rewrites it to the configured
ending. This is a deliberate divergence from yamllint, whose line layer cannot
see a bare `\r` and whose `type` has no `mac` value; on supported LF/CRLF files
the behaviour is identical. See
[Migrating from yamllint](../getting-started/migrating-from-yamllint.md#bare-carriage-return-r-line-breaks).

## Automatic fixing

`ryl check --fix` rewrites all line endings to match `type`. Disable with:

```toml
[lint]
fixable = ["ALL"]
unfixable = ["new-lines"]
```

`ryl format` rewrites line endings to `[format] line-ending`, `lf` by default. It conflicts with a `type` that resolves to a different ending, and with any `type` under `line-ending = "auto"`; see [Conflicting lint rules](../formatter.md#conflicting-lint-rules).

## Related rules

- [`new-line-at-end-of-file`](new-line-at-end-of-file.md) &mdash; controls
  the trailing newline at end of file.
- [`trailing-spaces`](trailing-spaces.md) &mdash; reports whitespace
  before the line ending.
