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
comments, does not pad flow collections, indents with two spaces, and emits LF line
endings. It does not canonicalize truthy values. Its settings are documented in the
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
- yamlfmt's default
  [`line_ending`](https://github.com/google/yamlfmt/blob/main/docs/config-file.md#basic-formatter)
  is OS-dependent (`crlf` on Windows, `lf` otherwise), so on Windows the `new-lines = unix`
  above would loop. Pin `line_ending: lf` in your `.yamlfmt` config to keep output LF on
  every platform (matching `new-lines = unix`).
- Leave `truthy` off (or expect warnings): yamlfmt keeps `yes`/`no`/`on`/`off` as written
  and ryl cannot rewrite them.

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
two spaces, emits LF, and canonicalizes truthy values (`yes` becomes `true`), so the
`truthy` rule stays clean. It removes the `...` document-end marker. Its settings are
documented in the [yamlfix configuration docs](https://lyz-code.github.io/yamlfix/).

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
| `quoted-strings` `quote-type` | any | not `single` | use `only-when-needed` |
| `new-lines` `type` | `unix` † | `unix` | `unix` |

† yamlfmt's default `line_ending` is `crlf` on Windows; set yamlfmt's `line_ending: lf`
so `new-lines = unix` holds cross-platform.

Two of these point in opposite directions across formatters, which is why there is no
single config that suits all three at once: flow-mapping padding (`braces`, 1 for
Prettier but 0 for the others) and inline-comment spacing (`comments`, 1 for yamlfmt and
Prettier but 2 for yamlfix). Pick the config for the formatter you actually use.

## Rules that never conflict

These rules govern things the formatters either produce the same way or leave untouched,
so you can enable them with any formatter:

- Layout ryl and the formatters agree on: `colons`, `commas`, `hyphens`, `brackets`
  (default), `new-line-at-end-of-file`, `trailing-spaces`, `empty-lines`,
  `comments-indentation`, `indentation` (two-space, sequences indented).
- Content rules the formatters do not touch: `anchors`, `tags`, `key-duplicates`,
  `merge-keys`, `empty-values`, `octal-values`, `float-values`,
  `block-scalar-chomping`, `unicode-line-breaks`. These report their construct if it is
  present; the formatter neither adds nor removes it.

## Rules that may warn without looping

These never loop, but a formatter will not fix what they flag, so they warn until you
align them or accept the finding:

- `truthy`: yamlfmt and Prettier leave `yes`/`no` as written; only yamlfix canonicalizes.
- `key-ordering`: no formatter reorders keys, so enable it only if your sources are
  already ordered.
- `line-length`: a formatter cannot break a long unbreakable scalar (such as a URL), so
  set `max` to suit your formatter's print width and expect occasional findings on long
  values.
