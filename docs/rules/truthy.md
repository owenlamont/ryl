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
[rules.truthy]
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
```

Quoting the key does not help. ryl resolves the YAML 1.2 core schema, where `on`
is an ordinary string, so `'on':` then trips
[`quoted-strings`](quoted-strings.md) under `required: only-when-needed`.
yamllint accepts the quotes because PyYAML reads `on` as a YAML 1.1 boolean
&mdash; see [YAML version compatibility](../yaml-version.md).

Three ways to settle it, narrowest first:

| Approach | Effect |
| :--- | :--- |
| [`per-file-ignores`](../file-ignores.md) on the workflow directory | `truthy` still runs everywhere else. Recommended |
| `check-keys = false` | No key is checked, in any file |
| `on` in `allowed-values` | Also permits `on` as a *value*, in any file |

The recommended form scopes the exemption to the files that need it:

```toml
[rules.truthy]

[per-file-ignores]
".github/workflows/*" = ["truthy"]
```

Both alternatives are global, which is the trade-off to weigh: they are simpler
to write, and they stop `truthy` catching an accidental `enabled: yes` in the
same repo.


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
```

### :white_check_mark: Allowed (with `allowed-values: ["true", "false", "yes", "no"]`)

```yaml
enabled: yes
visible: no
```

## YAML version directive

The rule honours an explicit [`%YAML` directive](../yaml-version.md): under
`%YAML 1.2` the barewords resolve to plain strings, so only `true`/`false`
spellings are flagged; under `%YAML 1.1` (or no directive) the full 1.1
truthy word list is flagged.

## Automatic fixing

This rule does not auto-fix; replacing a bareword changes the value's
type and meaning, which requires intent from the author.

## Related rules

- [`quoted-strings`](quoted-strings.md) &mdash; quoting a value disables
  type coercion so the literal stays a string.
- [`empty-values`](empty-values.md) &mdash; a related class of "what did
  the author actually mean" ambiguity.
