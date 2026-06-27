# block-scalar-chomping

## What this rule does

Requires every literal (`|`) or folded (`>`) block scalar to carry an explicit
**chomping indicator** &mdash; `-` (strip) or `+` (keep):

```yaml
script: |-
  echo "hello"
```

A bare `|`/`>`, or a header with only an indentation indicator such as `|2`, is
flagged because its chomping is still the implicit default. The rule is **off by
default**.

## Why this matters

A block scalar header may end with a chomping indicator that fixes how the
scalar's *trailing* line breaks are handled (YAML 1.2.2
[§8.1.1.2](https://spec.yaml.io/main/spec/1.2.2/#8112-block-chomping-indicator)):

- `-` &mdash; **strip**: remove every trailing line break.
- `+` &mdash; **keep**: keep every trailing line break.
- *(none)* &mdash; **clip**: keep exactly one final line break. This is the
  default a bare `|`/`>` selects.

The clip default is implicit and easy to forget, so whether a block scalar ends
with a newline (or how many) silently depends on a detail that is not written
down. That trailing newline frequently matters &mdash; an embedded shell script,
a certificate, a key, or a templated config can behave differently with or
without it. Spelling the indicator out documents the author's intent at the point
it is decided.

An indentation indicator on its own (`|2`) is **not** a chomping indicator, so it
is still flagged: the chomping remains the implicit clip default.

Note that YAML has **no explicit clip indicator** &mdash; only `-` and `+` exist.
This rule therefore asks you to choose deliberately between strip and keep rather
than rely on the silent clip default; it cannot be satisfied by "making clip
explicit", because there is no such spelling.

Sources: YAML 1.2.2 §8.1.1.2 (block chomping indicator);
[yaml.info block-scalar chomp examples](https://www.yaml.info/learn/quote#chomp).

## Configuration

`block-scalar-chomping` is a ryl-only rule (yamllint has no equivalent), so it is
configured **only in TOML** &mdash; `[rules.block-scalar-chomping]` in
`.ryl.toml`/`ryl.toml` or `[tool.ryl.rules.block-scalar-chomping]` in
`pyproject.toml`. It is rejected in yamllint-compatible YAML config (including `-d`
data) so the YAML namespace stays reserved for any future yamllint rule.

```toml
[rules.block-scalar-chomping]
level = "error"
```

The rule has no options: when enabled it requires an explicit `-`/`+` on every
literal/folded block scalar.

## Examples

### :x: Reported

```yaml
clip: |
  one line
folded: >
  some prose
indent_only: |2
  body
```

```text
1:7   error  missing explicit chomping indicator ("-" or "+")  (block-scalar-chomping)
3:9   error  missing explicit chomping indicator ("-" or "+")  (block-scalar-chomping)
5:14  error  missing explicit chomping indicator ("-" or "+")  (block-scalar-chomping)
```

### :white_check_mark: Allowed

```yaml
strip: |-
  no trailing newline
keep: >+
  every trailing newline kept
```

## Automatic fixing

This rule does not auto-fix. YAML has no explicit clip indicator, so a bare
`|`/`>` cannot be annotated without switching it to strip (`-`) or keep (`+`)
&mdash; which changes the scalar's trailing newlines and therefore its resolved
value. Choosing between strip and keep is the author's intent, so no single
rewrite is universally safe.

## Related rules

- [`indentation`](indentation.md) &mdash; its `check-multi-line-strings` option
  governs the *indentation* of block scalar content; this rule governs the
  *chomping indicator* in the header.
