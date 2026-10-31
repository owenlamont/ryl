# schema-dependent-values

## What this rule does

Reports plain scalars that YAML 1.1 and the YAML 1.2 core schema read as
different values, in a document with no `%YAML` directive.

| Option | Kind | Example | YAML 1.2 | YAML 1.1 |
| :--- | :--- | :--- | :--- | :--- |
| `forbid-timestamps` | Dates and timestamps | `2026-10-10` | string | timestamp |
| `forbid-sexagesimal` | Base-60 numbers | `22:22` | string | integer 1342 |
| `forbid-underscored-numbers` | Numbers with `_` or `0b` | `1_000`, `0b1010` | string | integer |
| `forbid-leading-zero-decimals` | A leading zero before `8` or `9` | `08` | integer 8 | string |
| `forbid-single-letter-booleans` | `y`, `Y`, `n`, `N` | `y` | string | boolean |

Keys are checked as well as values. Quoted and tagged scalars are never
reported.

Other rules own the rest of the divergent spellings:
[`truthy`](truthy.md) (`yes`, `no`, `on`, `off`),
[`octal-values`](octal-values.md) (`0777`, `0o777`) and
[`float-values`](float-values.md) (`1e5`, with `forbid-scientific-notation`).

## Why this matters

ryl reads a directive-less document as YAML 1.2, but PyYAML, ruamel.yaml's
`%YAML 1.1` mode and go-yaml read some of these spellings as YAML 1.1 types:

| Scalar | PyYAML | ruamel.yaml (default) | go-yaml v2 | go-yaml v3 |
| :--- | :--- | :--- | :--- | :--- |
| `2026-10-10` | date | date | string | `time.Time` |
| `22:22` | 1342 | string | string | string |
| `1_000` | 1000 | 1000 | 1000 | 1000 |
| `08` | string | 8 | float 8 | float 8 |
| `y` | string | string | boolean | string |

Quote the value if it is a string, or rewrite it to a spelling both versions read
alike.

## Configuration

`schema-dependent-values` is configured **only in TOML** &mdash;
`[lint.rules.schema-dependent-values]` in `.ryl.toml`/`ryl.toml` or
`[tool.ryl.lint.rules.schema-dependent-values]` in `pyproject.toml`. YAML config
(including `-d` data) rejects it.

```toml
[lint.rules.schema-dependent-values]
level = "error"
forbid-timestamps = true
forbid-sexagesimal = true
forbid-underscored-numbers = true
forbid-leading-zero-decimals = true
forbid-single-letter-booleans = true
```

Every option defaults to `true`.

The rule reports only where a document must mean the same to both versions: it
is quiet under [`target-version = "1.2"`](../yaml-version.md#how-ryl-handles-yaml-versions)
and in a document that declares `%YAML 1.1` or `%YAML 1.2`, which names one
version. These are the same scalars `ryl format` keeps quoted by default, so
formatting and this rule never disagree.

## Examples

```console
$ cat config.yaml
released: 2026-10-10
port: 22:22
budget: 1_000
zone: 08
enabled: y
$ ryl check config.yaml
config.yaml
  1:11      error    plain "2026-10-10" is a string in YAML 1.2 but a timestamp in YAML 1.1  (schema-dependent-values)
  2:7       error    plain "22:22" is a string in YAML 1.2 but a sexagesimal number in YAML 1.1  (schema-dependent-values)
  3:9       error    plain "1_000" is a string in YAML 1.2 but a number in YAML 1.1  (schema-dependent-values)
  4:7       error    plain "08" is an integer in YAML 1.2 but a string in YAML 1.1  (schema-dependent-values)
  5:10      error    plain "y" is a string in YAML 1.2 but a boolean in YAML 1.1  (schema-dependent-values)
```

## Automatic fixing

None. Quoting, retyping or rewriting the value each needs the author's intent:
`22:22` may be a time of day or a port mapping, and `08` a zero-padded string or the
number 8.

## Related rules

- [`truthy`](truthy.md), [`octal-values`](octal-values.md),
  [`float-values`](float-values.md) &mdash; the other spellings that differ.
- [`yaml-directive`](yaml-directive.md) &mdash; requires a directive, after which
  each document names its own version.
