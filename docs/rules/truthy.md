# truthy

## What this rule does

Reports bareword string values that YAML 1.1 would interpret as booleans.
By default only `true` and `false` are accepted; all other YAML 1.1
truthy words (`yes`, `no`, `on`, `off`, `True`, `Yes`, ...) are flagged.

## Why this matters

- **Silent type coercion.** A literal `yes` parses as the boolean `true`
  under YAML 1.1, which is rarely what an author intended for a country
  code, a country name, or a configuration value.
- **Cross-parser drift.** Some libraries still default to YAML 1.1
  semantics, others to 1.2; flagging the ambiguous words makes the
  document behave the same way everywhere.

## Configuration

```toml
[lint.rules.truthy]
level = "error"
allowed-values = ["true", "false"]
check-keys = true
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `allowed-values` | `["true", "false"]` | Bareword values that are permitted. Everything else triggers the rule. |
| `check-keys` | `true` | Also report truthy values used as mapping keys. |

## GitHub Actions workflows

A workflow's required `on:` key is a truthy bareword, so `truthy` reports it:

```console
$ ryl check .github/workflows/ci.yml
.github/workflows/ci.yml
  2:1       error    truthy value should be one of [false, true]  (truthy)

1 unsafe fix available with `--fix --unsafe-fixes`.
```

Quoting the key also works: `'on': push` passes both `truthy` and
[`quoted-strings`](quoted-strings.md), which keeps quotes needed by YAML 1.1 readers.
See [YAML version compatibility](../yaml-version.md) for how implicit types
affect quoting.

Three ways to settle it, narrowest first:

| Approach | Effect |
| :--- | :--- |
| [`per-file-ignores`](../file-ignores.md) on the workflow directory | `truthy` still runs everywhere else. Recommended |
| `check-keys = false` | No key is checked, in any file |
| `on` in `allowed-values` | Also permits `on` as a *value*, in any file |

The recommended form scopes the exemption to the files that need it:

```toml
[lint.rules.truthy]

[lint.per-file-ignores]
".github/workflows/*" = ["truthy"]
```

Both alternatives apply globally: `check-keys = false` skips all keys, while allowing
`on` permits `on` in keys and values. Both still report `enabled: yes`.


## Examples

### :white_check_mark: Allowed (defaults)

```yaml
enabled: true
visible: false
```

### :x: Reported (defaults)

```yaml
enabled: yes
flag: On
country: NO
debug: True
```

### :wrench: After `ryl check --fix` (defaults)

```yaml
enabled: yes
flag: On
country: NO
debug: true
```

### :white_check_mark: Allowed (with `allowed-values: ["true", "false", "yes", "no"]`)

```yaml
enabled: yes
visible: no
```

## YAML version directive

The rule honours an explicit [`%YAML` directive](../yaml-version.md): under
`%YAML 1.2` the barewords resolve to plain strings, so only `true`/`false`
spellings are flagged; under `%YAML 1.1` the full 1.1 truthy word list is
flagged. A document with no directive follows the top-level
[`target-version`](../yaml-version.md#how-ryl-handles-yaml-versions): the 1.1
list by default, the 1.2 one under `target-version = "1.2"`.

## Automatic fixing

`ryl check --fix` rewrites a flagged case variant of `true` or `false`
(`True`, `TRUE`, `False`, `FALSE`) to the first spelling of the same boolean
that `allowed-values` permits, trying lowercase, then title case, then upper
case. Every one of those spellings is the same boolean under YAML 1.1 and 1.2,
so the value does not change. When `allowed-values` has no spelling of that
boolean, the value is left as it is.

`yes`, `no`, `on`, `off` and their case variants get an
[unsafe fix](../rules.md#unsafe-fixes) instead, because the right replacement
(quote it to keep the string, or change it to a boolean) depends on what the
author meant. It applies only in a document with no `%YAML` directive under the
default `target-version = "1.1"`, and follows
[`source-version`](../yaml-version.md#unsafe-fixes-and-source-version):
`enabled: yes` becomes `enabled: "yes"` under `"1.2"` (the default) and
`enabled: true` under `"1.1"`, taking the first spelling of that boolean
`allowed-values` permits, or none when it permits none.

With `check-keys` on, keys are fixed too. That can surface a duplicate key:
`True: 1` and `true: 2` in one mapping were always the same boolean key, and
[`key-duplicates`](key-duplicates.md) reports it once both read `true`.

## Related rules

- [`quoted-strings`](quoted-strings.md) &mdash; quoting a value disables
  type coercion so the literal stays a string.
- [`empty-values`](empty-values.md) &mdash; a related class of "what did
  the author actually mean" ambiguity.
