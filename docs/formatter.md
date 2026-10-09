# Formatter

`ryl format` rewrites YAML files in place to a consistent layout. The formatter edits
only the spans that are off-style rather than reprinting the document, so the first run
on an existing project produces a small diff, and every later run on formatted files
changes nothing.

## `ryl format`

```bash
# Format every YAML file under the current directory, in place
ryl format .

# Write nothing; exit 1 if any file would change, listing why
ryl format --check .

# Write nothing; print a unified diff of the changes
ryl format --diff path/to/file.yaml

# Read stdin, write the formatted text to stdout
cat file.yaml | ryl format - --stdin-filename file.yaml
```

Inputs, file discovery, `.gitignore` handling and config discovery are the same as
`ryl check`. Unlike `ryl check`, `ryl format` needs no config: with none it formats
with the defaults below.

`--check` reports each line `ryl format` would change as a diagnostic from the rule
whose fix changes it, through the same reporters as `ryl check`, so it honours
[`[output]`](output-formats.md) and picks the GitHub format in GitHub Actions.

## Guarantees

Every edit `ryl format` makes is checked by property tests to be:

- **Value-preserving.** The document loads to the same data, with the same tags,
  anchors, aliases, key order and duplicate keys, resolved under the document's YAML
  version.
- **Comment-safe.** Every comment stays beside the node it annotates.
- **Idempotent.** Formatting formatted output changes nothing.

A file that does not parse is left alone and reported on stderr as
`skipped by ryl format`.

## Configuration

`ryl format` reads the `[format]` table of the same `.ryl.toml`, `ryl.toml` or
`pyproject.toml` (`[tool.ryl.format]`) that `ryl check` uses. A yamllint-style YAML
config cannot set it, so YAML-configured projects get the defaults.

```toml
[format]
quote-style = "single"
line-ending = "lf"
document-start = "preserve"
document-end = "preserve"
fold-long-lines = false
brace-spacing = false
comment-spacing = 2
comment-starting-space = "add"
max-blank-lines = 2
preview = false
sequence-style = "preserve"
mapping-style = "preserve"
indent-sequences = true
dash-on-own-line = false
```

| Key | Default | Values | What it sets |
| :--- | :--- | :--- | :--- |
| `quote-style` | `"single"` | `"single"`, `"double"`, `"preserve"` | The quote used where a string needs quoting; `"preserve"` leaves all quoting alone. See [Quote style](#quote-style). |
| `line-ending` | `"lf"` | `"lf"`, `"cr-lf"`, `"native"` | Line endings, including the final newline; `"native"` is the platform's. |
| `document-start` | `"preserve"` | `"add"`, `"preserve"` | Whether to add a missing `---` document start marker. |
| `document-end` | `"preserve"` | `"add"`, `"preserve"` | Whether to add a missing `...` document end marker. |
| `fold-long-lines` | `false` | `true`, `false` | Whether to split plain scalar lines longer than `line-length` at single spaces. See [Long lines](#long-lines). |
| `brace-spacing` | `false` | `true`, `false` | Whether to write one space inside non-empty flow mapping braces, `{ a: 1 }`. Empty braces and brackets stay unpadded. |
| `comment-spacing` | `2` | `1` to `255` | Exact number of spaces between content and an inline comment. |
| `comment-starting-space` | `"add"` | `"add"`, `"preserve"` | Whether to add a missing space after a comment's `#`. |
| `max-blank-lines` | `2` | `0` to `255` | Most blank lines kept in a row between content lines. |
| `indent-sequences` | `true` | `true`, `false` | Whether a block sequence under a mapping key is indented past the key; `false` keeps it flush. |
| `dash-on-own-line` | `false` | `true`, `false` | Whether a block mapping in a block sequence starts on the line after its `-`; `false` joins them as `- name: web`. |
| `preview` | `false` | `true`, `false` | Opt in to style changes before they become stable. See [Preview style](#preview-style). |
| `sequence-style` | `"preserve"` | `"preserve"`, `"block"`, `"flow"` | Collection style for sequences. See [Collection style](#collection-style). |
| `mapping-style` | `"preserve"` | `"preserve"`, `"block"`, `"flow"` | Collection style for mappings. See [Collection style](#collection-style). |

The top-level `line-length` and `indent-width` keys are shared with `ryl check`, where
they set the defaults for the `line-length` and `indentation` rules (see
[Quick start](getting-started/quickstart.md#configure-for-your-project)). `ryl format`
re-indents every block level to `indent-width`, and uses both as the fold width and
continuation indent under `fold-long-lines`.

The rest of the layout is fixed:

| Concern | `ryl format` writes |
| :--- | :--- |
| Own-line comments | Aligned with the content they precede |
| Flow collections | No spaces inside `[]` or empty `{}`, and inside other `{}` per `brace-spacing`; no space before a comma, one after |
| Blank lines | None at the start or end of the file |
| Line ends | No trailing whitespace; exactly one newline at the end of the file |
| After `-`, `?` and `:` | One space, re-indenting a compact collection that hangs on it |

A document that a tab indents, or that re-indenting would parse differently, is left as
it is and named on stderr.

## Default profile

With no `[format]` table, `ryl format` writes the profile below. Every default but
`document-start` agrees with yamllint's `default` preset: `ryl check --fix` under that
preset only adds a missing `---` to the output.

| Default | Why |
| :--- | :--- |
| `quote-style = "single"` | yamlfix's default; see [Quote style](#quote-style) for when it applies |
| `line-ending = "lf"` | The default of biome, prettier and yamllint's `new-lines` |
| `document-start = "preserve"` | prettier keeps a file's markers as written |
| `document-end = "preserve"` | No formatter adds `...`; prettier keeps it |
| `fold-long-lines = false` | Prose keeps its line breaks, as under prettier's `proseWrap: "preserve"` |
| `brace-spacing = false` | yamllint's `braces` defaults and yamlfix; prettier and biome pad |
| `comment-spacing = 2` | yamllint's `comments`, ruff and yamlfix |
| `comment-starting-space = "add"` | yamllint's `comments` requires the space |
| `max-blank-lines = 2` | yamllint's `empty-lines` and ruff |
| `sequence-style = "preserve"` | Restyling a collection is opt-in |
| `mapping-style = "preserve"` | Restyling a collection is opt-in |
| `indent-sequences = true` | yamllint's `indentation` default; prettier indents them too |
| `dash-on-own-line = false` | prettier writes `- name: web` |
| `preview = false` | Preview styles are opt-in, as in ruff |
| `line-length = 80` | The default of prettier, biome and yamllint's `line-length` |
| `indent-width = 2` | The default of prettier, biome and yamlfix |
| Fixed layout above | Every formatter probed agrees |

## Long lines

With `fold-long-lines = true`, `ryl format` splits a block plain scalar line longer than
the top-level `line-length` (default 80) at a single space. The new line takes the
indent of the scalar's existing continuation lines, or else sits `indent-width` past the
column of the collection that owns the value:

```yaml
# before
items:
  - description: This single line is well over eighty characters wide and so the formatter should fold it.
```

```yaml
# after, with fold-long-lines = true
---
items:
  - description: This single line is well over eighty characters wide and so the
      formatter should fold it.
```

Keys, flow collections, quoted and block scalars, and words with no space to break at
stay as they are. A line ending in an inline directive comment stays whole. Folding
ignores the `line-length` rule's own options, and `ryl check --fix` never folds.

## Collection style

With `sequence-style` or `mapping-style` set to `"block"`, `ryl format` rewrites flow
collections of that kind in block style, indented by `indent-width`, copying each
entry's text. An inline comment after the collection moves to its key's line:

```yaml
# before
items: [one, two]  # shopping
metadata: {name: example}
```

```yaml
# after, with both set to "block"
---
items:  # shopping
  - one
  - two
metadata:
  name: example
```

Empty `[]` and `{}` stay flow. A collection that holds a comment, a multi-line entry, a
collection or empty key, or an entry some loaders would read differently in block style
stays as written, and `ryl format` says why on stderr.

With `"flow"`, a block collection of scalars and aliases that is not the document root
becomes one flow line, such as `items: [one, two]`, when it holds no comment and the
line fits `line-length`. Block scalars, multi-line or empty entries, and plain scalars
holding `,[]{}` or starting with `:` or `?` keep it block. Flow collections already
written are left alone, even long ones.

## Quote style

`ryl format` quotes a string only where its plain form would load as something else,
using `quote-style`:

```yaml
# before
a: "x"
b: "123"
c: "it's"
d: "tab\there"
e: 'it''s: x'
f: "say \"hi\": x"
```

```yaml
# after, with quote-style = "single"
---
a: x
b: '123'
c: it's
d: "tab\there"
e: "it's: x"
f: 'say "hi": x'
```

A string that needs quoting takes whichever quotes avoid an escape: single when it
contains `"`, double when it contains only `'`, else `quote-style`. A string that needs
escape sequences stays double-quoted. A string that YAML 1.1 loads
as a boolean, such as `"yes"`, `"NO"` or `"on"`, keeps its quotes, so `truthy` stays
clean. Mapping keys follow the same rules.

## Preview style

`preview = true` opts in to style changes before they become stable. As in ruff, the
stable style changes only in a minor release, by promoting a preview style, while a
preview style can change in any release. A fix for invalid output, a changed value or a
lost comment can ship in any release. No preview styles exist yet, so `preview` changes
nothing today.

## YAML in Markdown

`ryl format` formats YAML front matter and fenced YAML blocks in Markdown files matched
by `[files].markdown`, as described in [YAML in Markdown](markdown.md). There is no
`--markdown` flag on `ryl format`; set `[files].markdown` instead.

## Suppression

`ryl format` honours the same [inline directives](directives.md) as `ryl check`: a
`# yamllint disable-file` comment leaves the file alone, and
`# yamllint disable rule:quoted-strings` or `disable-line rule:quoted-strings` stops
that rule's formatting where it applies. There is no format-only directive.

## Conflicting lint rules

The formatter's layout is also checked by lint rules, and they stay lint rules:
`ryl check` still reports them, and `ryl check --fix` still fixes the fixable ones. When
one of them is enabled with options that reject what `ryl format` writes, every
`ryl format` run warns on stderr, for example:

```text
warning: the quoted-strings lint rule's options are incompatible with the formatter's `[format] quote-style = "single"`. Disable quoted-strings when using `ryl format`, or set its options `quote-type = "single"`, `required = "only-when-needed"`, `allow-double-quotes-for-escaping = true`, `allow-quoted-quotes = true`.
```

`--no-warnings` silences it. A rule left at its defaults conflicts only where the table
says so.

| Rule | Conflicts with `ryl format` when |
| :--- | :--- |
| [`braces`](rules/braces.md) | `forbid` is set and `[format] mapping-style` is not `"preserve"`, `min-spaces-inside-empty` is above 0, or `min-spaces-inside` to `max-spaces-inside` excludes the `brace-spacing` padding (0, or 1 when `true`) |
| [`brackets`](rules/brackets.md) | `forbid` is set and `[format] sequence-style` is not `"preserve"`, or `min-spaces-inside` or `min-spaces-inside-empty` is above 0 |
| [`commas`](rules/commas.md) | `min-spaces-after` is above 1, or `max-spaces-after` is 0 |
| [`comments`](rules/comments.md) | `min-spaces-from-content` is above `[format] comment-spacing`, or `max-spaces-from-content` is below it (`-1`, the default, is unlimited); or `ignore-shebangs = false` while `require-starting-space = true` under `[format] comment-starting-space = "add"`, since a shebang keeps its `#!` |
| [`comments-indentation`](rules/comments-indentation.md) | Never |
| [`document-start`](rules/document-start.md) | `present = false` with `[format] document-start = "add"` |
| [`document-end`](rules/document-end.md) | `present = false` with `[format] document-end = "add"` |
| [`empty-lines`](rules/empty-lines.md) | `max` is below `[format] max-blank-lines` |
| [`new-line-at-end-of-file`](rules/new-line-at-end-of-file.md) | Never |
| [`new-lines`](rules/new-lines.md) | `type` resolves to a different ending from `[format] line-ending` |
| [`quoted-strings`](rules/quoted-strings.md) | See below; never under `quote-style = "preserve"` |
| [`trailing-spaces`](rules/trailing-spaces.md) | Never |
| [`colons`](rules/colons.md) | `max-spaces-after` is 0 |
| [`hyphens`](rules/hyphens.md) | `max-spaces-after` is 0, or `dash-on-own-line = true` while `[format] dash-on-own-line = false` |
| [`indentation`](rules/indentation.md) | `spaces` is a number other than `indent-width`, or `indent-sequences` is the opposite of `[format] indent-sequences`, or `check-multi-line-strings = true` with an `indent-width` other than 2, since block scalar bodies keep their indent |
| [`line-length`](rules/line-length.md) | Never; folding is opt-in and leaves lines it cannot break |

`quoted-strings` accepts the formatter's output when `required` is `"only-when-needed"`
or `false`, `extra-required` is empty, and `quote-type` is either `"any"` or matches
`quote-style`. A `quote-type` of `"single"` or `"double"` also needs
`allow-quoted-quotes = true`, for strings quoted to avoid an escape, and `"single"`
needs `allow-double-quotes-for-escaping = true`, for the strings that need escapes. The rule's
defaults (`required = true`) conflict:

<!-- ryl-config-check: format-conflict -->
```toml
[lint.rules]
quoted-strings = "enable"
```

These quoted-strings configs accept the formatter's output:

<!-- ryl-config-check: format-clean -->
```toml
[lint.rules.quoted-strings]
quote-type = "any"
required = "only-when-needed"
```

<!-- ryl-config-check: format-clean -->
```toml
[lint.rules.quoted-strings]
quote-type = "single"
required = "only-when-needed"
allow-double-quotes-for-escaping = true
allow-quoted-quotes = true
```

<!-- ryl-config-check: format-clean -->
```toml
[format]
quote-style = "double"

[lint.rules.quoted-strings]
quote-type = "double"
required = "only-when-needed"
allow-quoted-quotes = true
```

A YAML config cannot set `allow-double-quotes-for-escaping`, so there use
`quote-type: any`:

<!-- ryl-config-check: format-clean -->
```yaml
rules:
  quoted-strings:
    quote-type: any
    required: only-when-needed
```

The other conflicts follow the table:

<!-- ryl-config-check: format-conflict -->
```toml
[format]
document-start = "add"

[lint.rules.document-start]
present = false
```

<!-- ryl-config-check: format-clean -->
```toml
[lint.rules.document-start]
present = false
```

<!-- ryl-config-check: format-clean -->
```toml
[format]
brace-spacing = true

[lint.rules.braces]
min-spaces-inside = 1
max-spaces-inside = 1
min-spaces-inside-empty = 0
max-spaces-inside-empty = 0
```

<!-- ryl-config-check: format-clean -->
```toml
[lint.rules.commas]
max-spaces-after = -1

[lint.rules.comments]
max-spaces-from-content = -1
```

The other eleven rules check things `ryl format` never changes, so they never conflict
with it: [`anchors`](rules/anchors.md),
[`block-scalar-chomping`](rules/block-scalar-chomping.md),
[`empty-values`](rules/empty-values.md), [`float-values`](rules/float-values.md),
[`key-duplicates`](rules/key-duplicates.md), [`key-ordering`](rules/key-ordering.md),
[`merge-keys`](rules/merge-keys.md), [`octal-values`](rules/octal-values.md),
[`tags`](rules/tags.md), [`truthy`](rules/truthy.md) and
[`unicode-line-breaks`](rules/unicode-line-breaks.md). `truthy` and `key-ordering` keep
their `ryl check --fix` fixes.

## Recommended lint config

To lint alongside `ryl format`, start from the
[recommended starter config](getting-started/quickstart.md#lint-alongside-ryl-format),
which enables only rules the formatter never touches and leaves layout to
`ryl format --check`.

## Exit codes

| Code | `ryl format` | `ryl format --check` / `--diff` |
| :--- | :--- | :--- |
| `0` | Files formatted, or nothing to do | No file would change |
| `1` | Not used | At least one file would change |
| `2` | A usage or config error, an unreadable file, or a file no `[files]` glob matches | Same |

A file skipped for a syntax error does not change the exit code.
