# hyphens

## What this rule does

Controls the number of spaces between the hyphen (`-`) and the value in
block sequences.

## Why this matters

- **Alignment.** Consistent spacing keeps sequence items lined up under
  the same column, which matters for readability of long lists.
- **Diff stability.** Editors that "fix" hyphen spacing during a save
  produce noisy diffs when the rule is unset.

## Configuration

```toml
[lint.rules.hyphens]
level = "error"
max-spaces-after = 1
dash-on-own-line = false
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `max-spaces-after` | `1` | Maximum spaces between the `-` and the item value. |
| `dash-on-own-line` | `false` | Require the `-` on its own line when the entry is a block mapping (ryl-only; TOML config only). |

`dash-on-own-line` is a ryl-only extension with no yamllint counterpart, so it is
configured in TOML config only and rejected in yamllint-compatible YAML config.

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
list:
  - first
  - second
```

### :x: Reported (defaults)

```yaml
list:
  -  first
  -   second
```

### :white_check_mark: Allowed (with `max-spaces-after: 3`)

```yaml
list:
  -   first
  -   second
```

### :x: Reported (with `dash-on-own-line: true`)

The mapping starts on the dash's line, so the `-` is not on its own line:

```yaml
items:
  - name: web
    port: 80
```

### :white_check_mark: Allowed (with `dash-on-own-line: true`)

The `-` stands alone and the mapping body is indented below it (a dash carrying
only an anchor/tag or a comment is also accepted, since the keys remain below):

```yaml
items:
  -
    name: web
    port: 80
```

## Automatic fixing

`ryl check --fix` trims the spaces after `-` to `max-spaces-after`, never below one.
`ryl format` leaves exactly one. Both leave a `-` alone when the block mapping or
sequence it opens continues on later lines, and `ryl format` reports it: the spaces set
that collection's indentation, so trimming them alone would re-indent or break it:

```yaml
-   name: web    # left alone: `port` is aligned under `name`
    port: 80
-   - a          # left alone
    - b
```

`dash-on-own-line` has no automatic fix: breaking the mapping onto the line below the
`-` re-indents its body.

## Related rules

- [`indentation`](indentation.md) &mdash; controls how sequence items are
  indented relative to their parent key.
- [`brackets`](brackets.md) and [`commas`](commas.md) &mdash; the flow
  sequence equivalents.
