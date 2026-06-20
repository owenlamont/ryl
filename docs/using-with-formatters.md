# Using ryl with a YAML formatter

The boundary between linting and formatting YAML is blurry. Many of ryl's rules are about
layout (spacing, indentation, quote style, blank lines), and ryl already applies safe
`--fix` edits to a good number of them, so it does some formatting today. What ryl does
not do, for now, is reflow a whole document into one canonical layout the way a dedicated
formatter does. Some projects therefore run ryl alongside a formatter such as
[google/yamlfmt](https://github.com/google/yamlfmt),
[Prettier](https://prettier.io/), or [yamlfix](https://lyz-code.github.io/yamlfix/) for
that canonical layout, and rely on ryl for the broader checks (and safe fixes) it adds on
top.

When two tools both rewrite files they can disagree. A setting in one can undo a fix from
the other, and running them in a loop (for example in a pre-commit hook) makes them fight:
the formatter changes a byte, `ryl --fix` changes it back, the formatter changes it again.
This page lists the ryl settings that matter for each formatter and gives a verified,
conflict-free starting config for each.

## How conflicts happen, and why they are avoidable

ryl's TOML configuration has no default rule set: every rule is off until you enable it
explicitly. That is what makes coexistence simple, because you only ever opt in to the
rules you want, and you can match their settings to whatever your formatter produces.

There are exactly two ways the two tools can disagree:

- **A loop.** Only a rule with a safe fix can take part, because a loop needs both tools
  to edit the same construct. ryl's fixable rules are `braces`, `brackets`, `commas`,
  `comments`, `comments-indentation`, `document-start`, `document-end`, `empty-lines`,
  `new-line-at-end-of-file`, `new-lines`, `quoted-strings`, and `trailing-spaces`. If one
  of these is set to enforce the opposite of what your formatter emits, they fight.
- **A standing complaint.** A rule with no fix (for example `indentation`, `line-length`,
  `truthy`) can flag something the formatter produced but ryl cannot rewrite. There is no
  loop, but ryl warns on every run until you align the setting or turn the rule off.

Everything below is about steering clear of both. The configs were checked by running
`formatter` then `ryl --fix` repeatedly until the file settled, and confirming the
settled file passes `ryl` with no findings.

!!! note "Check mode vs fix mode"

    A few alignments rely on `ryl --fix` applying a one-time fix that the formatter then
    keeps (for example ryl adding `---` where the formatter preserves it). If you run ryl
    in lint-only mode (`ryl` with no `--fix`, common in CI) rather than fix mode, prefer
    the settings marked as needing no fix below, or run `ryl --fix` once before the check.

## google/yamlfmt

yamlfmt strips the `---` document-start marker, uses a single space before inline
comments, does not pad flow collections, indents with two spaces, and writes the
platform's native line endings (CRLF on Windows; see
[Line endings across operating systems](#line-endings-across-operating-systems)). It does
not canonicalize truthy values. Its settings are documented in the
[yamlfmt config-file reference](https://github.com/google/yamlfmt/blob/main/docs/config-file.md).

```toml
# .ryl.toml, tuned for google/yamlfmt
[rules]
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
comments-indentation = "enable"
hyphens = "enable"
new-line-at-end-of-file = "enable"
trailing-spaces = "enable"

[rules.document-start]
present = false              # yamlfmt removes `---`

[rules.comments]
min-spaces-from-content = 1  # yamlfmt uses one space before inline comments

[rules.new-lines]
type = "unix"

[rules.empty-lines]
max = 2

[rules.indentation]
spaces = 2
indent-sequences = true

[rules.quoted-strings]
required = "only-when-needed"

[rules.line-length]
max = 120
```

Notes:

- If you prefer to keep `---`, set yamlfmt's
  [`include_document_start: true`](https://github.com/google/yamlfmt/blob/main/docs/config-file.md#basic-formatter)
  in its `.yamlfmt` config and change ryl to `[rules.document-start]` `present = true`. The
  two markers must agree. yamlfmt strips the `...` document-end marker, so leave ryl's
  `document-end` rule off.
- Leave `truthy` off (or expect warnings): yamlfmt keeps `yes`/`no`/`on`/`off` as written
  and ryl cannot rewrite them.
- On Windows, pin yamlfmt's `line_ending: lf` so the recipe's `new-lines` `type = "unix"`
  holds (see
  [Line endings across operating systems](#line-endings-across-operating-systems)).

## Prettier

Prettier pads flow mappings as `{ a: 1 }` but does not pad flow sequences, uses a single
space before inline comments, preserves `---` if present but never adds it, indents with
two spaces, and emits LF. It does not canonicalize truthy values. Prettier has no
YAML-specific options; the general options that affect YAML output are
[`printWidth`](https://prettier.io/docs/options#print-width),
[`bracketSpacing`](https://prettier.io/docs/options#bracket-spacing) (the flow-mapping
padding this recipe relies on), [`singleQuote`](https://prettier.io/docs/options#quotes),
[`proseWrap`](https://prettier.io/docs/options#prose-wrap), and
[`endOfLine`](https://prettier.io/docs/options#end-of-line).

```toml
# .ryl.toml, tuned for Prettier
[rules]
brackets = "enable"
colons = "enable"
commas = "enable"
comments-indentation = "enable"
hyphens = "enable"
new-line-at-end-of-file = "enable"
trailing-spaces = "enable"

[rules.braces]
min-spaces-inside = 1       # Prettier pads `{ a: 1 }`
max-spaces-inside = 1

[rules.comments]
min-spaces-from-content = 1  # Prettier uses one space before inline comments

[rules.new-lines]
type = "unix"

[rules.empty-lines]
max = 2

[rules.indentation]
spaces = 2
indent-sequences = true

[rules.quoted-strings]
required = "only-when-needed"

[rules.line-length]
max = 120
```

Notes:

- `document-start` is left off here. Prettier never adds or removes `---`, and ryl's
  document-start fix can add markers but not strip them, so `present = false` would
  permanently flag any file that already has a `---`. To enforce consistent markers
  instead, set `present = true` and run `ryl --fix` (ryl adds `---`, Prettier keeps it).
- The flow-padding split is the key alignment: `braces` must allow one inner space, while
  `brackets` must allow none. Setting either `braces` or `brackets` to `forbid` clashes
  with Prettier, which keeps short collections in flow style.
- `quoted-strings` must not require single quotes: Prettier normalizes to double quotes,
  so `quote-type = "single"` loops. `only-when-needed` (or double) is safe.
- Leave `truthy` off (or expect warnings): Prettier does not canonicalize truthy values.

## yamlfix

yamlfix is the most ryl-aligned of the three. It adds `---`, uses two spaces before
inline comments (the same as ryl's default), does not pad flow collections, indents with
two spaces, and canonicalizes truthy values (`yes` becomes `true`), so the `truthy` rule
stays clean. It removes the `...` document-end marker. Like yamlfmt it writes the
platform's native line endings (CRLF on Windows; see
[Line endings across operating systems](#line-endings-across-operating-systems)). Its
settings are documented in the
[yamlfix configuration docs](https://lyz-code.github.io/yamlfix/).

```toml
# .ryl.toml, tuned for yamlfix
[rules]
braces = "enable"
brackets = "enable"
colons = "enable"
commas = "enable"
comments-indentation = "enable"
hyphens = "enable"
new-line-at-end-of-file = "enable"
trailing-spaces = "enable"
truthy = "enable"           # yamlfix canonicalizes yes/no to true/false

[rules.document-start]
present = true              # yamlfix adds `---`

[rules.comments]
min-spaces-from-content = 2  # yamlfix uses two spaces (ryl's default)

[rules.new-lines]
type = "unix"

[rules.empty-lines]
max = 2

[rules.indentation]
spaces = 2
indent-sequences = true

[rules.quoted-strings]
required = "only-when-needed"

[rules.line-length]
max = 120
```

Notes:

- Keep `document-end` off: yamlfix does not emit `...`, and ryl's `document-start` fix
  adds markers but never removes them, so requiring `...` would warn on every file.
- `quoted-strings` must not require quotes: yamlfix strips unnecessary quotes, so
  `required = true` loops. Use `only-when-needed`.

## Settings that need the most care

These are the rule settings that loop with at least one formatter. Match them to your
formatter (the recipes above already do):

| ryl setting | yamlfmt | Prettier | yamlfix |
|---|---|---|---|
| `document-start` `present` | `false` | off (or `true` + `--fix`) | `true` |
| `document-end` `present` | off | off | off |
| `braces` inner spaces | `0` | `1` | `0` |
| `brackets` inner spaces | `0` | `0` | `0` |
| `comments` `min-spaces-from-content` | `1` | `1` | `2` |
| `quoted-strings` | `required = "only-when-needed"` | `required = "only-when-needed"`, never `quote-type = "single"` | `required = "only-when-needed"` |
| `new-lines` `type` | `unix` † | `unix` | `unix` † |

† yamlfmt and yamlfix emit the platform's native line endings (CRLF on Windows); see
[Line endings across operating systems](#line-endings-across-operating-systems).

Two of these point in opposite directions across formatters, which is why there is no
single config that suits all three at once: flow-mapping padding (`braces`, 1 for
Prettier but 0 for the others) and inline-comment spacing (`comments`, 1 for yamlfmt and
Prettier but 2 for yamlfix). Pick the config for the formatter you actually use.

## Line endings across operating systems

Every recipe above enables `new-lines` with `type = "unix"`, which expects LF. Prettier
always writes LF, but **yamlfmt and yamlfix write the platform's native line endings, so
on Windows they emit CRLF** and fight that `unix` setting: ryl rewrites the file to LF,
the formatter rewrites it back to CRLF. Fixes:

- **yamlfmt** has a line-ending option: set
  [`line_ending: lf`](https://github.com/google/yamlfmt/blob/main/docs/config-file.md#basic-formatter)
  in `.yamlfmt` and the recipe's `type = "unix"` holds on every OS.
- **yamlfix** has no line-ending option, so either set `[rules.new-lines]` to
  `type = "platform"` (ryl then accepts the local convention and never fights yamlfix) or
  drop the `new-lines` rule, and normalize the committed form at the git layer with a
  `.gitattributes` entry such as `*.yaml text eol=lf`.
- **Prettier** writes LF on every OS (its `endOfLine` defaults to `lf`), so no change is
  needed.

On Linux and macOS all three already emit LF, so the `new-lines` `type = "unix"` setting
is conflict-free there as written.

## Rules the formatter output already satisfies

The formatters produce output that meets these rules, so enabling them adds no findings
on formatted files: `colons`, `commas`, `hyphens`, `brackets` (default spacing),
`new-line-at-end-of-file`, `trailing-spaces`, `empty-lines`, `comments-indentation`, and
`indentation` (two-space, sequences indented).

## Rules that may warn without looping

None of these loop, but a formatter will not fix what they flag, so they warn until you
align them or accept the finding. They land here for one of two reasons:

- The formatter produces the disfavoured form: `truthy` (yamlfmt and Prettier leave
  `yes`/`no` as written; only yamlfix canonicalizes), `key-ordering` (no formatter
  reorders keys, so enable it only if your sources are already ordered), and
  `line-length` (a formatter cannot break a long unbreakable scalar such as a URL, so set
  `max` to suit your print width and expect occasional findings on long values).
- The rule checks content the formatter is neutral about, neither adding nor removing the
  construct: `empty-values`, `octal-values`, `float-values`, `anchors`, `merge-keys`,
  `block-scalar-chomping`, `key-duplicates`, `tags`, and `unicode-line-breaks`. These
  behave exactly as they would with no formatter: each reports its construct if your
  source contains it, and no formatter will fix it for you. Enable them as linting
  choices, independent of your formatter.
