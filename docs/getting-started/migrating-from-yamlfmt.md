# Migrating from google/yamlfmt

Replace `yamlfmt .` with `ryl format .` and `yamlfmt -lint .` with
`ryl format --check .`. The settings below approximate the default basic formatter
in google/yamlfmt 0.21.0. They were compared on the same YAML inputs with
`yamlfmt -in` and `ryl format -`.

## Example

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

| yamlfmt setting | ryl equivalent |
| --- | --- |
| `indent: 2` | top-level `indent-width = 2` |
| `pad_line_comments: 1` | `[format] comment-spacing = 1` |
| `line_ending: lf` | `[format] line-ending = "lf"` (default) |
| `include_document_start: false` | `[format] document-start = "preserve"`; see [markers](#document-markers) |
| `max_line_length: 0` | `[format] fold-long-lines = false` (default) |

## Deliberate differences

### Document markers

yamlfmt removes `---` and `...` from a single document in the tested default
profile. ryl preserves existing markers: removing one can change a multi-document
stream or discard an explicit boundary. `document-start = "add"` adds missing
`---` if the project requires it.

### Layout and values

yamlfmt reprints the document, including removing blank lines. ryl edits the
existing text and keeps any construct that cannot be safely restyled. In the
comparison, yamlfmt kept existing flow collections and quote styles; ryl's
`sequence-style = "preserve"` and `mapping-style = "preserve"` do likewise.
For inputs involving tags, aliases, or unusual scalars, run `ryl format --diff .`
before replacing the formatter: ryl retains the parsed values, comments, and
anchors rather than matching yamlfmt's reprint at any cost.
