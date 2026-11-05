# Migrating from google/yamlfmt

Import the basic formatter settings first (a successful import exits `0`; inspect warnings):

```bash
ryl --migrate-configs --migrate-from yamlfmt --migrate-stdout
ryl --migrate-configs --migrate-from yamlfmt --migrate-write
```

Follow [After migrating](migrating-from-formatters.md#after-migrating) to verify
formatting, convert comments and update CI, or use the manual profile below.

The [config importer](migrating-from-formatters.md) imports `exclude` when
`match_type: doublestar`, anchoring patterns to the configuration directory.
The importer cleans relative path components and rebases absolute paths inside
that directory; paths outside the directory and bare directory exclusions warn.
Yamlfmt matches the paths produced by its include globs from the invocation
directory, so use the configuration directory as the migration root.
Standard matching, `regex_exclude`, and `include` settings still warn.

Replace `yamlfmt .` with `ryl format .` and `yamlfmt -lint .` with
`ryl format --check .`. The settings below approximate the default basic formatter
in google/yamlfmt 0.21.0. They were compared on the same YAML inputs with
`yamlfmt -in` and `ryl format -`.

ryl defaults to indent width 2. Set top-level `indent-width` explicitly to keep
another width.

## Manual configuration

```toml
# .ryl.toml
indent-width = 2

[format]
quote-style = "preserve"
comment-spacing = 1
max-blank-lines = 0
```

`quote-style = "preserve"` keeps existing quotes, as yamlfmt did in the comparison.
`comment-spacing = 1` matches its inline comments. Its basic formatter removes
blank lines between entries; `max-blank-lines = 0` does the same. Both indent
sequences under mapping keys by two spaces and use LF in the tested environment.
See the [formatter settings](../formatter.md#configuration) for other options.

## Option mapping

`formatter.*` below names keys nested under `formatter:`, rather than top-level keys:

```yaml
formatter:
  indent: 4
```

| yamlfmt setting | ryl equivalent |
| --- | --- |
| `formatter.indent: 2` | top-level `indent-width = 2` |
| `formatter.pad_line_comments: 1` | `[format] comment-spacing = 1` |
| `formatter.line_ending: lf` | `[format] line-ending = "lf"` (default) |
| `formatter.line_ending: crlf` | `[format] line-ending = "cr-lf"` |
| `formatter.include_document_start: false` (default) | `[format] document-start = "remove"`; see [markers](#document-markers) |
| `formatter.include_document_start: true` | `[format] document-start = "add"` |
| `formatter.max_line_length: 0` | `[format] fold-long-lines = false` (default) |
| `formatter.max_line_length: 40` | Approximately: top-level `line-length = 40` and `[format] fold-long-lines = true` |

At 40 columns, yamlfmt wraps plain scalars differently and wraps flow sequences that ryl leaves on one line unless `sequence-expand` is set.

## Deliberate differences

### Document markers

yamlfmt removes `---` and `...` from a single document in the tested default
profile. Imported `document-start = "remove"` removes the first optional `---`;
directives, empty documents and separators keep required markers. Existing `...`
markers remain. `document-start = "add"` adds missing `---` if required.

### Layout and values

yamlfmt reprints the document, including removing blank lines. ryl edits the
existing text and keeps any construct that cannot be safely restyled. In the
comparison, yamlfmt kept existing flow collections and quote styles; ryl's
`sequence-style = "preserve"` and `mapping-style = "preserve"` do likewise.
For inputs involving tags, aliases, or unusual scalars, run `ryl format --diff .`
before replacing the formatter: ryl retains the parsed values, comments, and
anchors rather than matching yamlfmt's reprint at any cost.
