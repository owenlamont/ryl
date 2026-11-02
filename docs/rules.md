# ryl Rules Reference

## A comprehensive reference of all YAML linting rules

## Introduction

ryl implements 29 rules for checking YAML files. This page is a categorised
index of every rule with a brief description and a link to its detailed
documentation. Each rule page covers what the rule does, why it matters,
configuration options, and (where applicable) automatic fix behaviour.

For configuration discovery, presets, and file selection, see
[Configuration presets](config-presets.md).

## Rule categories

- [Layout and spacing](#layout-and-spacing) &mdash; whitespace, indentation,
  line endings, line length
- [Document structure](#document-structure) &mdash; document markers, empty
  values, anchors, tags, keys
- [Comments](#comments) &mdash; comment placement and spacing
- [Values](#values) &mdash; numeric, string, and boolean value formats

Rules that auto-fix are marked with :wrench: in the **Fix** column, and rules with an
[unsafe fix](#unsafe-fixes) with :warning:. Most layout
rules are also enforced by [`ryl format`](formatter.md); the
[Formatter](formatter.md#conflicting-lint-rules) page lists which, and the options that
conflict with it.

## Layout and spacing

| Rule | Description | Fix |
| :--- | :--- | :---: |
| [`block-scalar-chomping`](rules/block-scalar-chomping.md) | Explicit chomping indicator (`-`/`+`) on block scalars. |  |
| [`braces`](rules/braces.md) | Spaces inside flow mapping braces (`{...}`). | :wrench: |
| [`brackets`](rules/brackets.md) | Spaces inside flow sequence brackets (`[...]`). | :wrench: |
| [`colons`](rules/colons.md) | Spaces around mapping colons. | :wrench: |
| [`commas`](rules/commas.md) | Spaces around flow collection commas. | :wrench: |
| [`empty-lines`](rules/empty-lines.md) | Number of consecutive empty lines. | :wrench: |
| [`hyphens`](rules/hyphens.md) | Spaces after sequence hyphens. | :wrench: |
| [`indentation`](rules/indentation.md) | Block indentation, sequence indentation, multi-line strings. |  |
| [`line-length`](rules/line-length.md) | Maximum line length. |  |
| [`new-line-at-end-of-file`](rules/new-line-at-end-of-file.md) | A trailing newline at end of file. | :wrench: |
| [`new-lines`](rules/new-lines.md) | Consistent line endings (LF vs CRLF). | :wrench: |
| [`trailing-spaces`](rules/trailing-spaces.md) | Trailing whitespace at end of lines. | :wrench: |
| [`unicode-line-breaks`](rules/unicode-line-breaks.md) | Raw NEL / LS / PS characters (not YAML 1.2 line breaks). |  |

## Document structure

| Rule | Description | Fix |
| :--- | :--- | :---: |
| [`anchors`](rules/anchors.md) | Anchor and alias declarations and usage. |  |
| [`document-end`](rules/document-end.md) | Document end marker `...`. | :wrench: |
| [`document-start`](rules/document-start.md) | Document start marker `---`. | :wrench: |
| [`empty-values`](rules/empty-values.md) | Empty values in mappings and sequences. |  |
| [`key-duplicates`](rules/key-duplicates.md) | Duplicate keys in mappings. |  |
| [`key-ordering`](rules/key-ordering.md) | Alphabetical or configured ordering of mapping keys. | :wrench: |
| [`merge-keys`](rules/merge-keys.md) | The `<<` merge key (a YAML 1.1 feature removed in 1.2). |  |
| [`tags`](rules/tags.md) | Unsafe and non-portable YAML tags. |  |
| [`yaml-directive`](rules/yaml-directive.md) | A `%YAML` version directive on every document. | :wrench: |

## Comments

| Rule | Description | Fix |
| :--- | :--- | :---: |
| [`comments`](rules/comments.md) | Spaces after `#` and before inline comments. | :wrench: |
| [`comments-indentation`](rules/comments-indentation.md) | Comment alignment with surrounding content. | :wrench: |

## Values

| Rule | Description | Fix |
| :--- | :--- | :---: |
| [`float-values`](rules/float-values.md) | Float value formats. |  |
| [`octal-values`](rules/octal-values.md) | Octal value formats. | :warning: |
| [`quoted-strings`](rules/quoted-strings.md) | Quoted string styles and when to require quotes. | :wrench: |
| [`schema-dependent-values`](rules/schema-dependent-values.md) | Plain values YAML 1.1 and 1.2 read differently (`2026-10-10`, `22:22`, `1_000`). | :warning: |
| [`truthy`](rules/truthy.md) | Truthy values like `yes`, `no`, `on`, `off`. | :wrench: :warning: |

## Severity levels

Warnings return exit code `0` unless strict mode is enabled with `--strict` or
TOML [`[lint] strict = true`](getting-started/quickstart.md#strict-mode).
Use `--no-strict` to override a configured strict mode for one run.

Every rule can be configured to report at either `error` (the default for
most rules) or `warning`. Errors cause `ryl` to exit non-zero; warnings are
printed and fail the run only under strict mode.

Configure the severity inline with the rule:

```toml
[lint.rules.line-length]
level = "warning"
max = 120
```

## Enabling and disabling rules

Toggle a rule on or off with a top-level string in the `[lint.rules]` table:

```toml
[lint.rules]
truthy = "disable"
key-ordering = "enable"
```

Enabling a rule without options applies its defaults. To enable every rule,
including the ryl-only ones, use `ALL`:

```toml
[lint.rules]
ALL = "enable"
truthy = "disable"

[lint.rules.line-length]
max = 120
```

Each rule then runs with its own defaults at level `error`. An explicit entry for
a rule wins over `ALL` wherever it appears in the table. `ALL` is TOML-only;
yamllint-compatible YAML config rejects it.

!!! warning "`ALL` can grow when you upgrade ryl"

    `ALL`, in config or as `--enable ALL`, turns on every rule the installed ryl
    has, including any added after you wrote the config. An upgrade that adds a rule
    can then fail a check that passed before. Pin the ryl version, or list the rules
    you want.

`ryl check --enable <RULES>` picks the rules from the command line instead:
comma-separated rule IDs or `ALL`, and the flag can be repeated. It replaces the
config's selection, so only the listed rules run. A listed rule keeps the options
and level the config gives it, and runs at its defaults if the config disables
it or does not mention it. `--disable <RULES>` then subtracts comma-separated rule IDs
or `ALL` from the selection; pass `--disable` once per command. `ignore`,
`per-file-ignores` and inline directives
still apply. The flag works without any config file:

```bash
ryl check --enable ALL .
ryl check --enable truthy,line-length .
```

The built-in `default`
and `relaxed` presets cover the common starting points (the `empty` preset is
YAML-`extends:` only, with no usable TOML form); see
[Configuration presets](config-presets.md).

To switch rules off for part of a file with `# ryl disable` /
`# yamllint disable` comments, see [Inline directives](directives.md).

## Automatic fixing

The `--fix` flag applies safe fixes for rules marked with :wrench: above:

```bash
ryl check --fix .
```

Control which rules apply fixes with `fixable`/`unfixable` in the `[lint]` table:

```toml
[lint]
fixable = ["ALL"]
unfixable = ["comments"]
```

### Unsafe fixes

A safe fix never changes what a value means. An **unsafe** fix may: it picks one
meaning where readers disagree, so it applies only when you opt in, as with ruff.
`truthy`, `octal-values` and `schema-dependent-values` offer one for a plain scalar
that YAML 1.1 and 1.2 readers load differently (see
[YAML version compatibility](yaml-version.md#unsafe-fixes-and-source-version)).

`ryl check` counts the fixes it left out:

```console
$ ryl check config.yaml
config.yaml
  1:10      error    truthy value should be one of [false, true]  (truthy)

1 unsafe fix available with `--fix --unsafe-fixes`.
```

Apply every unsafe fix with `--unsafe-fixes` or the top-level setting, or one rule's
with `extend-safe-fixes`, which lets plain `--fix` apply it:

```toml
unsafe-fixes = true

[lint]
extend-safe-fixes = ["octal-values"]
```

`--diff` previews them, and once the config enables a fix its message names the old
and new spelling (`; unsafe fix: yes -> "yes"`). `--no-unsafe-fixes` overrides the setting,
and `unsafe-fixes = false` also silences the count. `unfixable` blocks a rule's unsafe
fix too, and `ryl format` never applies one. In an editor, `ryl server` offers each
rule's unsafe fixes as a quick fix, kept out of "Fix all" until the config applies
them.
