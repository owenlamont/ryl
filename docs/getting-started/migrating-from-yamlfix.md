# Migrating from yamlfix

`ryl format` covers most of [yamlfix](https://github.com/lyz-code/yamlfix)'s
options through `[format]` keys and the top-level `line-length` and
`indent-width`. Replace `yamlfix .` with `ryl format .`, and `yamlfix --check .`
with `ryl format --check .`.

ryl reads configuration from `.ryl.toml`, `ryl.toml` or `[tool.ryl]` in
`pyproject.toml`, or the file passed with `-c`. It does not read `YAMLFIX_*`
environment variables.

## Example

A yamlfix configuration:

```toml
[tool.yamlfix]
line_length = 100
explicit_start = false
whitelines = 1
comments_min_spaces_from_content = 1
```

becomes:

```toml
[tool.ryl]
line-length = 100

[tool.ryl.format]
fold-long-lines = true
document-start = "preserve"
max-blank-lines = 1
comment-spacing = 1
```

## Option mapping

| yamlfix option (default) | ryl equivalent |
| --- | --- |
| `comments_min_spaces_from_content` (2) | `[format] comment-spacing` (default 2) |
| `comments_require_starting_space` (true) | `[format] comment-starting-space = "add"`; `false` maps to `"preserve"` |
| `whitelines` (0) | `[format] max-blank-lines` (default 2), see [blank lines](#blank-lines) |
| `comments_whitelines` (1) | none, see [blank lines](#blank-lines) |
| `section_whitelines` (0) | none, see [blank lines](#blank-lines) |
| `explicit_start` (true) | `[format] document-start = "add"`; `false` maps to `"preserve"` |
| `indent_mapping` (2) | top-level `indent-width` |
| `indent_sequence` (4), `indent_offset` (2) | planned in [#383](https://github.com/owenlamont/ryl/issues/383) |
| `line_length` (80) | top-level `line-length` with `[format] fold-long-lines = true` |
| `preserve_quotes` (false) | `true` maps to `[format] quote-style = "preserve"`; for `false`, `quote-style = "single"` or `"double"` is the closest match, see [forcing quotes](#forcing-quotes) |
| `quote_representation` (`'`) | `[lint.rules.quoted-strings] quote-type = "single"` or `"double"`, see [forcing quotes](#forcing-quotes) |
| `quote_basic_values`, `quote_keys_and_basic_values` (false) | see [forcing quotes](#forcing-quotes) |
| `none_representation` (`""`) | none: ryl keeps each null as written |
| `sequence_style` (`flow_style`) | planned in [#446](https://github.com/owenlamont/ryl/issues/446) |
| `allow_duplicate_keys` (false) | none: not a formatting concern; the `key-duplicates` lint rule reports them |
| `config_path` | none: ryl discovers its own config, or `-c` takes a translated config file |

## Differences in behaviour

### Blank lines

- yamlfix's `whitelines = N` sets every run of blank lines to exactly N, so it
  expands an existing run that is shorter. ryl's `max-blank-lines` only caps a
  run.
- ryl has no separate count before comment lines (`comments_whitelines`), and
  never adds blank lines around multi-line top-level entries
  (`section_whitelines`).

### Comment spacing

yamlfix only changes the gap before an inline comment when
`comments_min_spaces_from_content` is above 1. ryl's `comment-spacing` always
sets the gap exactly, so `comment-spacing = 1` also shrinks a wider gap.

### Removing `---`

yamlfix's `explicit_start = false` removes document start markers, which merges
the documents of a multi-document stream. ryl never removes `---`;
`document-start = "preserve"` leaves markers as written.

### Forcing quotes

ryl's formatter quotes only where a value needs quotes. To quote every plain
string, as yamlfix's `quote_basic_values` does, enable the `quoted-strings`
lint rule with `required = true` and fix with `ryl check --fix`, and set
`[format] quote-style = "preserve"` so `ryl format` keeps the quotes. The
rule's `quote-type` picks the quote, as `quote_representation` does:

```toml
[format]
quote-style = "preserve"

[lint.rules.quoted-strings]
required = true
quote-type = "single"
```

`quote_representation` only applies when force-quoting. ryl's
`[format] quote-style` instead sets the quote for every string that needs one,
so it approximates yamlfix's quoting rather than matching it.

Add `check-keys = true` to quote keys too, as `quote_keys_and_basic_values`
does.

### Rewrites ryl does not do

- yamlfix rewrites `yes`, `on` and similar to `true`/`false`. Under YAML 1.2
  those are strings, so the rewrite changes values; ryl leaves them.
- yamlfix escapes Jinja `{{ }}` templates. ryl refuses YAML it cannot parse.
- yamlfix's `line_length` also joins lines the author broke. ryl only splits
  lines longer than `line-length`, and only when `fold-long-lines` is on.
