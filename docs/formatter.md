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
document-start = "add"
document-end = "preserve"
fold-long-lines = false
preview = false
```

| Key | Default | Values | What it sets |
| :--- | :--- | :--- | :--- |
| `quote-style` | `"single"` | `"single"`, `"double"`, `"preserve"` | The quote used where a string needs quoting; `"preserve"` leaves all quoting alone. See [Quote style](#quote-style). |
| `line-ending` | `"lf"` | `"lf"`, `"cr-lf"`, `"native"` | Line endings, including the final newline; `"native"` is the platform's. |
| `document-start` | `"add"` | `"add"`, `"preserve"` | Whether to add a missing `---` document start marker. |
| `document-end` | `"preserve"` | `"add"`, `"preserve"` | Whether to add a missing `...` document end marker. |
| `fold-long-lines` | `false` | `true`, `false` | Whether to split plain scalar lines longer than `line-length` at single spaces. See [Long lines](#long-lines). |
| `preview` | `false` | `true`, `false` | Opt in to style changes before they become stable. See [Preview style](#preview-style). |

The top-level `line-length` and `indent-width` keys are shared with `ryl check`, where
they set the defaults for the `line-length` and `indentation` rules (see
[Quick start](getting-started/quickstart.md#configure-for-your-project)). `ryl format`
uses both as the fold width and continuation indent under `fold-long-lines`.

The rest of the layout is fixed:

| Concern | `ryl format` writes |
| :--- | :--- |
| Inline comments | Two spaces before `#`; own-line comments aligned with the content they precede |
| Flow collections | No spaces inside `{}` or `[]`; no space before a comma, one after |
| Blank lines | At most two in a row; none at the start or end of the file |
| Line ends | No trailing whitespace; exactly one newline at the end of the file |

`ryl format` does not yet rewrite colon or hyphen spacing or indentation.

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

## Quote style

`ryl format` quotes a string only where its plain form would load as something else,
using `quote-style`:

```yaml
# before
a: "x"
b: "123"
c: "it's"
d: "tab\there"
```

```yaml
# after, with quote-style = "single"
---
a: x
b: '123'
c: it's
d: "tab\there"
```

A string that needs escape sequences stays double-quoted. A string that YAML 1.1 loads
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
warning: the quoted-strings lint rule's options are incompatible with the formatter's `[format] quote-style = "single"`. Disable quoted-strings when using `ryl format`, or change its options to accept the formatter's output.
```

`--no-warnings` silences it. A rule left at its defaults conflicts only where the table
says so.

| Rule | Conflicts with `ryl format` when |
| :--- | :--- |
| [`braces`](rules/braces.md), [`brackets`](rules/brackets.md) | `forbid` is set, or `min-spaces-inside` or `min-spaces-inside-empty` is above 0 |
| [`commas`](rules/commas.md) | `min-spaces-after` is above 1, or `max-spaces-after` is 0 |
| [`comments`](rules/comments.md) | `min-spaces-from-content` is above 2, or `max-spaces-from-content` is below 2 (`-1`, the default, is unlimited) |
| [`comments-indentation`](rules/comments-indentation.md) | Never |
| [`document-start`](rules/document-start.md) | `present = false`, unless `[format] document-start = "preserve"` |
| [`document-end`](rules/document-end.md) | `present = false` with `[format] document-end = "add"` |
| [`empty-lines`](rules/empty-lines.md) | `max` is below 2 |
| [`new-line-at-end-of-file`](rules/new-line-at-end-of-file.md) | Never |
| [`new-lines`](rules/new-lines.md) | `type` resolves to a different ending from `[format] line-ending` |
| [`quoted-strings`](rules/quoted-strings.md) | See below; never under `quote-style = "preserve"` |
| [`trailing-spaces`](rules/trailing-spaces.md) | Never |
| [`colons`](rules/colons.md), [`hyphens`](rules/hyphens.md), [`indentation`](rules/indentation.md) | Never, because `ryl format` does not rewrite their concerns yet |
| [`line-length`](rules/line-length.md) | Never; folding is opt-in and leaves lines it cannot break |

`quoted-strings` accepts the formatter's output when `required` is `"only-when-needed"`
or `false`, `extra-required` is empty, and `quote-type` is either `"any"` or matches
`quote-style`. A `quote-type` of `"single"` also needs
`allow-double-quotes-for-escaping = true`, for the strings that need escapes. The rule's
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
```

<!-- ryl-config-check: format-clean -->
```toml
[format]
quote-style = "double"

[lint.rules.quoted-strings]
quote-type = "double"
required = "only-when-needed"
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
[lint.rules.document-start]
present = false
```

<!-- ryl-config-check: format-clean -->
```toml
[format]
document-start = "preserve"

[lint.rules.document-start]
present = false
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
