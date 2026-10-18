# Migrating from Prettier

For YAML files, run `ryl format .` to write changes and
`ryl format --check .` in CI. Keep Prettier for other file types. The settings below follow
Prettier 3.9.10's YAML output on the same inputs used with `ryl format -`.
Prettier's [general options](https://prettier.io/docs/options) can alter the
starting profile.

ryl defaults to indent width 2. Set top-level `indent-width` explicitly to keep
another width.

## Example

```toml
# .ryl.toml
indent-width = 2

[format]
quote-style = "double"
brace-spacing = true
comment-spacing = 1
max-blank-lines = 1
```

`brace-spacing = true` produces `{ a: 1 }` while leaving `{}` tight.
`comment-spacing = 1` matches inline comments; `max-blank-lines = 1` matches
the tested collapse of a longer blank run. Both tools indent nested sequences
by two spaces and write LF in the tested profile.

## Option mapping

| Prettier option (default) | ryl equivalent |
| --- | --- |
| `tabWidth: 2` | top-level `indent-width = 2` |
| `singleQuote: false` | `[format] quote-style = "double"`; see [quotes](#quotes) |
| `bracketSpacing: true` | `[format] brace-spacing = true` |
| `endOfLine: "lf"` | `[format] line-ending = "lf"` (default) |
| `printWidth: 80` | top-level `line-length = 80` plus `[format] fold-long-lines = true` is only an approximation; see [long lines](#long-lines) |

## Deliberate differences

### Quotes

Prettier keeps the quotes around `"hello"`; ryl removes them when the scalar
can be plain, even with `quote-style = "double"`. The setting chooses double
quotes only for strings that still need quoting. Use `quote-style = "preserve"`
if retaining existing quotes matters more than normalizing their style.

### Long lines

Prettier expands a long flow sequence into multiple lines at its print width.
ryl preserves the flow sequence by default. `fold-long-lines = true` folds
eligible scalar text at `line-length`; it cannot reproduce Prettier's collection
layout, and may fold scalars that Prettier keeps on one line.

### Layout and markers

Prettier reprints the document. ryl retains existing `---` and `...` markers,
flow or block collection style, comments, and anchors unless a safe formatting
pass changes them. Prettier also retains markers in the tested input, but the
two formatters may choose different whitespace around the same YAML value.
