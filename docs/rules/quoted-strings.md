# quoted-strings

## What this rule does

Enforces a consistent quoting style for string scalars and controls when
quotes are required.

The rule answers three questions:

1. **Which quote type is preferred** &mdash; single, double, or either.
2. **When must a string be quoted** &mdash; always, only when YAML would
   otherwise interpret it as a non-string type, or never.
3. **Which scalars are in scope** &mdash; values only, or both values and
   mapping keys.

## Why this matters

- **Avoid accidental retyping.** Bareword values like `yes` or `1.0` are
  silently parsed as booleans or floats. Quoting prevents this.
- **Consistent diffs.** Mixing `'foo'` and `"foo"` in the same file causes
  noisy diffs whenever an editor or formatter normalises the style.
- **Escapes and interpolation.** Single-quoted YAML scalars do not process
  escape sequences; double-quoted ones do. Picking the right type per
  project avoids surprises with `\n`, `\t`, and Unicode escapes.

## Examples

The examples below assume `quote-type = "double"` and
`required = "only-when-needed"` (the recommended starting point).

### :white_check_mark: Allowed

```yaml
title: A plain bareword string
flag: "yes"            # quoted because yes would parse as a boolean
escape: "line1\nline2" # quoted because it uses an escape sequence
```

### :x: Reported

```yaml
flag: yes              # unintentional boolean
title: 'A plain string'  # single-quoted when double is configured
```

### :wrench: After `ryl --fix`

```yaml
flag: "yes"
title: "A plain string"
```

## Configuration

```toml
[rules.quoted-strings]
level = "warning"
quote-type = "any"
required = true
extra-required = []
extra-allowed = []
allow-quoted-quotes = false
allow-double-quotes-for-escaping = false  # ryl-only
check-keys = false
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `quote-type` | `"any"` | `"single"`, `"double"`, or `"any"`. The quote style required when the rule decides a string must be quoted. |
| `required` | `true` | `true` &mdash; every string scalar must be quoted. `false` &mdash; no string scalar may be quoted. `"only-when-needed"` &mdash; require quotes only when leaving the scalar bare would change its YAML type. |
| `extra-required` | `[]` | Regular expressions; values matching any pattern must be quoted regardless of `required`. |
| `extra-allowed` | `[]` | Regular expressions; values matching any pattern may be quoted even when `required = false`. |
| `allow-quoted-quotes` | `false` | Permit single quotes inside a single-quoted string (and analogously for doubles) instead of forcing a switch to the other quote type. |
| `allow-double-quotes-for-escaping` | `false` | **ryl-only.** When `quote-type = "single"` and `required = "only-when-needed"`, allow double quotes specifically for strings that need an escape sequence. |
| `check-keys` | `false` | Also apply the rule to mapping keys, not only values. |

## Automatic fixing

`ryl --fix` rewrites string scalars to use the configured `quote-type` and
adds or removes quotes to satisfy `required`. The fix is conservative: it
only changes scalars where the corrected form parses to the same value as
the original.

Disable the fix for this rule by adding it to `[fix].unfixable`:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["quoted-strings"]
```

## Related rules

- [`truthy`](truthy.md) &mdash; complements `quoted-strings` by reporting
  bareword booleans that would otherwise need quoting.
- [`empty-values`](empty-values.md) &mdash; controls how missing values are
  written, which interacts with whether nulls should be quoted.
