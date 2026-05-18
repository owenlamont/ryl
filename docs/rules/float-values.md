# float-values

## What this rule does

Constrains how floating-point literals are written. Optionally rejects
implicit leading-decimal notation, scientific notation, and the special
values `.nan` and `.inf`.

## Why this matters

- **Parser compatibility.** Not every YAML consumer recognises `.nan` or
  `.inf`; forbidding them keeps documents portable.
- **Readability.** `.5` and `5e10` are valid YAML but can be misread.
  Requiring `0.5` and an explicit form makes intent obvious.

## Configuration

```toml
[rules.float-values]
level = "error"
require-numeral-before-decimal = false
forbid-scientific-notation = false
forbid-nan = false
forbid-inf = false
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `require-numeral-before-decimal` | `false` | Require a digit before the decimal point (forbid `.5`, accept `0.5`). |
| `forbid-scientific-notation` | `false` | Forbid scientific notation like `1e6`. |
| `forbid-nan` | `false` | Forbid `.nan` literals. |
| `forbid-inf` | `false` | Forbid `.inf` and `-.inf` literals. |

## Examples

### :white_check_mark: Allowed (with `require-numeral-before-decimal: true`)

```yaml
factor: 0.5
ratio: 0.0
```

### :x: Reported (with `require-numeral-before-decimal: true`)

```yaml
factor: .5
```

### :x: Reported (with `forbid-nan: true` and `forbid-inf: true`)

```yaml
not-a-number: .nan
infinity: .inf
```

## Automatic fixing

This rule does not auto-fix; rewriting the literal requires care to
preserve the intended value.

## Related rules

- [`octal-values`](octal-values.md) &mdash; the analogous rule for
  integer literals in octal notation.
- [`quoted-strings`](quoted-strings.md) &mdash; useful when you want to
  pin a value to a string regardless of how it would otherwise parse.
