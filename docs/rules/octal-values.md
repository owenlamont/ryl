# octal-values

## What this rule does

Reports octal integer literals. By default both YAML 1.1 implicit form
(`0755`, or signed `-0755`) and YAML 1.2 explicit form (`0o755`) are flagged.

## Why this matters

- **YAML 1.1 vs 1.2 mismatch.** `0755` is octal in YAML 1.1 (value 493)
  but decimal in YAML 1.2 (value 755). The same literal therefore means
  different things to different consumers.
- **Surprise.** Permissions or version numbers that begin with `0` are
  often intended as plain strings or decimal numbers, not octals.

## Configuration

```toml
[lint.rules.octal-values]
level = "error"
forbid-implicit-octal = true
forbid-explicit-octal = true
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `forbid-implicit-octal` | `true` | Forbid `0nnn` and `-0nnn` style literals that YAML 1.1 treats as octal. |
| `forbid-explicit-octal` | `true` | Forbid `0o755`-style literals that YAML 1.2 treats as octal. |

## Examples

### :white_check_mark: Allowed (defaults)

```yaml
permissions: "0755"
explicit: 493
```

### :x: Reported (with `forbid-implicit-octal: true`)

```yaml
permissions: 0755
```

### :x: Reported (with `forbid-explicit-octal: true`)

```yaml
permissions: 0o755
```

## Automatic fixing

No safe fix; rewriting an integer literal requires deciding whether the
intended value is octal, decimal, or a string. In a document with no `%YAML`
directive under the default `target-version = "1.1"`, an
[unsafe fix](../rules.md#unsafe-fixes) writes the decimal value
[`source-version`](../yaml-version.md#unsafe-fixes-and-source-version) reads:

| Literal | `source-version = "1.2"` (default) | `source-version = "1.1"` |
| :--- | :--- | :--- |
| `0755` | `755` | `493` |
| `-0755` | `-755` | `-493` |
| `0o755` | `493` | left alone |

YAML 1.1 readers disagree on `0o755` (a string to PyYAML), so it has no 1.1 fix.

## Related rules

- [`float-values`](float-values.md) &mdash; the analogous rule for
  floating-point literal formats.
- [`quoted-strings`](quoted-strings.md) &mdash; quoting a value pins it
  to a string regardless of how it would otherwise parse.
