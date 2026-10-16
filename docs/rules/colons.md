# colons

## What this rule does

Controls the number of spaces around mapping colons (`:`).

## Why this matters

- **Readability.** Consistent spacing makes columnar layouts easier to
  scan, especially in configuration files with many short keys.
- **Avoids ambiguity.** Stray spaces around the colon can hide subtle
  parsing bugs, particularly when keys contain values that look like
  flow-style content.

## Configuration

```toml
[lint.rules.colons]
level = "error"
max-spaces-before = 0
max-spaces-after = 1
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `max-spaces-before` | `0` | Maximum spaces between the key and the `:`. Use `-1` to disable. |
| `max-spaces-after` | `1` | Maximum spaces between the `:` and the value. Use `-1` to disable. |

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
key: value
object:
  - a
  - b
```

### :x: Reported (defaults)

```yaml
key : value
key:   value
```

### :white_check_mark: Allowed (with `max-spaces-after: 2`)

```yaml
first:  1
second: 2
third:  3
```

### Alias, anchor and tag mapping keys

A YAML anchor/alias name or a tag may legally contain `:`, so `*anchor:` welds into an
alias to an anchor named `anchor:` (a parse error here, since no mapping colon remains),
and `&anchor: value` is the scalar `value` anchored `anchor:`. Using an alias as a
mapping key, or an empty key carrying only an anchor or tag, therefore *requires* one
separating space before the colon &mdash; `*anchor : value`, `&anchor : value`. That one
space is not reported; more than one is reported as usual.

```yaml
base: &a name
*a : value     # allowed: the one required separating space is not reported
```

```yaml
base: &a name
*a  : value    # reported (2:4): too many spaces before colon
```

## Automatic fixing

`ryl check --fix` trims the spaces before `:` and after `:` or `?` to the configured
maximum, never below the one space an alias, anchor or tag key needs before `:`, or the
one space YAML needs after `:` or `?`. A space after `:` that leads to `,`, `]` or `}`
is left to [`commas`](commas.md), [`braces`](braces.md) and [`brackets`](brackets.md).

`ryl format` writes no space before `:` (one for those keys) and exactly one after `:`
and `?`, turning a tab into a space. An explicit `?` or `:` that opens a block
collection continuing below has its spaces set that collection's indentation, so
`ryl check --fix` leaves it alone and `ryl format` re-indents the collection with it:

```yaml
? key
:   - a      # ryl format: `: - a`, with `- b` under `- a`
    - b
```

## Related rules

- [`commas`](commas.md) &mdash; the analogous rule for flow collection
  commas.
- [`hyphens`](hyphens.md) &mdash; spacing for block sequence hyphens.
