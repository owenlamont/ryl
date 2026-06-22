# ryl Rules Reference

## A comprehensive reference of all YAML linting rules

## Introduction

ryl implements 27 rules for checking YAML files. This page is a categorised
index of every rule with a brief description and a link to its detailed
documentation. Each rule page covers what the rule does, why it matters,
configuration options, and (where applicable) automatic fix behaviour.

For configuration discovery, presets, and file selection, see
[Configuration presets](config-presets.md). For migrating from yamllint, see
[Migrating from yamllint](getting-started/migrating-from-yamllint.md).

## Rule categories

- [Layout and spacing](#layout-and-spacing) &mdash; whitespace, indentation,
  line endings, line length
- [Document structure](#document-structure) &mdash; document markers, empty
  values, anchors, tags, keys
- [Comments](#comments) &mdash; comment placement and spacing
- [Values](#values) &mdash; numeric, string, and boolean value formats

Rules that auto-fix are marked with :wrench: in the **Fix** column.

## Layout and spacing

| Rule | Description | Fix |
| :--- | :--- | :---: |
| [`block-scalar-chomping`](rules/block-scalar-chomping.md) | Explicit chomping indicator (`-`/`+`) on block scalars. |  |
| [`braces`](rules/braces.md) | Spaces inside flow mapping braces (`{...}`). | :wrench: |
| [`brackets`](rules/brackets.md) | Spaces inside flow sequence brackets (`[...]`). | :wrench: |
| [`colons`](rules/colons.md) | Spaces around mapping colons. |  |
| [`commas`](rules/commas.md) | Spaces around flow collection commas. | :wrench: |
| [`empty-lines`](rules/empty-lines.md) | Number of consecutive empty lines. | :wrench: |
| [`hyphens`](rules/hyphens.md) | Spaces after sequence hyphens. |  |
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
| [`key-ordering`](rules/key-ordering.md) | Alphabetical ordering of mapping keys. |  |
| [`merge-keys`](rules/merge-keys.md) | The `<<` merge key (a YAML 1.1 feature removed in 1.2). |  |
| [`tags`](rules/tags.md) | Unsafe and non-portable YAML tags. |  |

## Comments

| Rule | Description | Fix |
| :--- | :--- | :---: |
| [`comments`](rules/comments.md) | Spaces after `#` and before inline comments. | :wrench: |
| [`comments-indentation`](rules/comments-indentation.md) | Comment alignment with surrounding content. | :wrench: |

## Values

| Rule | Description | Fix |
| :--- | :--- | :---: |
| [`float-values`](rules/float-values.md) | Float value formats. |  |
| [`octal-values`](rules/octal-values.md) | Octal value formats. |  |
| [`quoted-strings`](rules/quoted-strings.md) | Quoted string styles and when to require quotes. | :wrench: |
| [`truthy`](rules/truthy.md) | Truthy values like `yes`, `no`, `on`, `off`. |  |

## Severity levels

Every rule can be configured to report at either `error` (the default for
most rules) or `warning`. Errors cause `ryl` to exit non-zero; warnings are
printed but do not fail the run.

Configure the severity inline with the rule:

```toml
[rules.line-length]
level = "warning"
max = 120
```

## Enabling and disabling rules

Toggle a rule on or off with a top-level string in the `[rules]` table:

```toml
[rules]
truthy = "disable"
key-ordering = "enable"
```

Enabling a rule without options applies its defaults. The built-in `default`
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

Control which rules apply fixes with a `[fix]` table:

```toml
[fix]
fixable = ["ALL"]
unfixable = ["comments"]
```
