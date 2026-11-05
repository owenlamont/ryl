# Inline directives

## Disabling rules from within a file

Sometimes a single line legitimately breaks a rule and changing the file is the
wrong fix. ryl supports inline comment directives that switch rules off for part
of a file. The preferred spelling uses `ryl`:

```yaml
key:   value  # ryl disable-line rule:colons
```

Every directive is an ordinary YAML comment, so it never changes how the
document parses.

## Forms

Rule directives have two scopes &mdash; a single line, or a block that runs until it is
re-enabled &mdash; and each can target all rules or a specific list.

### Single line

`disable-line` switches rules off for one line:

- As a **trailing** comment it applies to **its own line**.
- On **its own line** it applies to the **next** line.

```yaml
key:   value  # ryl disable-line rule:colons   # this line only

# ryl disable-line rule:colons
other:   value                                 # the line below the directive
```

### Block

`disable` switches rules off from its line onward; `enable` switches them back
on:

```yaml
# ryl disable rule:colons
a:   1                  # not reported
b:   2                  # not reported
# ryl enable rule:colons
c:   3                  # reported again
```

### Whole file

A `disable-file` directive on the **first line** of a file skips it entirely
&mdash; no rule reports anything (not even a syntax error), and `--fix` leaves it
untouched:

```yaml
# ryl disable-file
this:   file: is: not: linted
```

It must be the first line, with no `rule:` tokens. The `#`
may be followed by any spacing (`#ryl disable-file` is accepted too).
A first-line Jinja renderer header such as `#!jinja|yaml` skips a file the same way; see
[Jinja templates](formatter.md#jinja-templates).

### Targeting rules

List one or more rules with `rule:<id>` tokens (the bare rule ids ryl uses, e.g.
`colons`, `trailing-spaces`):

```yaml
value: yes  # ryl disable-line rule:truthy rule:colons
```

Omit the `rule:` tokens to affect **all** rules:

```yaml
# ryl disable        # mutes every rule …
messy :  [1 ,2 ]
# ryl enable         # … until here
```

### Alongside other comments

A `# ryl …` directive can share a comment with other text, before or after it,
such as a version pinned by Dependabot or Renovate or a reason for the
suppression:

```yaml
uses: actions/checkout@3d3c42e  # v4.1.0  # ryl disable-line rule:line-length
uses: actions/checkout@3d3c42e  # ryl disable-line rule:line-length  # v4.1.0
```

Each `#` preceded by whitespace starts a new part of the comment. A part that
matches the directive grammar is a directive; the others are ordinary comment
text, and their `rule:` tokens target nothing. This works for every form,
including a first-line `disable-file`, but only with the `ryl` spelling.

## yamllint compatibility

The `# yamllint …` spelling remains accepted throughout 0.29.x, though only as a
whole comment (see above). The CLI warns once per regular file or stdin input that
uses an alias; use `# ryl …` instead or convert comments with `--migrate-directives`.
`--no-warnings`
hides the notice. The notice does not affect lint results or strict-mode exit codes:

```yaml
key:   value  # yamllint disable-line rule:colons
```

Preview and apply conversion (`--migrate-from yamllint` is optional):

```bash
ryl --migrate-directives --migrate-from yamllint
ryl --migrate-directives --migrate-from yamllint --migrate-write
```

Both spellings follow the same grammar. A comment (or, for
`# ryl …`, one `#` part of it) is only treated as a directive when it matches
precisely &mdash; a single space after `#`, single
spaces between words, and `rule:` before each id. Near-misses are plain
comments and do **not** disable anything:

```yaml
a:   1  #   ryl disable-line rule:colons   # extra spaces → not a directive
a:   1  # ryl disable-line colons          # missing `rule:` → not a directive
a:   1  # v1  # yamllint disable-line rule:colons  # beside text → not a directive
```

`# yamllint …` is matched against the whole comment, as yamllint does, so it
can't share a comment with other text.

Rule directives do not suppress syntax errors; first-line `disable-file` skips the
file entirely.

## Interaction with `--fix`

`--fix` honours directives too: a fixer never rewrites a line whose rule is
disabled. Running `ryl check --fix` over the block above leaves `a:   1` and `b:   2`
untouched while still fixing `c:   3`.

## Formatter-only suppression

Use `# ryl format-ignore` on its own line to keep the next YAML node as written.
Formatter directives also accept no space after `#`, for example `#ryl format-ignore`.
A mapping key includes its value, and a collection includes its descendants:

```yaml
before: 0
# ryl format-ignore
section:
  first:    1
  second:    [1,2]
after: 3
```

Before a document's root node, `format-ignore` protects the entire root collection.
To protect an explicit region, use `format-disable` and `format-enable`:

```yaml
# ryl format-disable
manually:    [1,2]
# ryl format-enable
formatted: [1, 2]
```

An unmatched `format-disable` lasts until the document ends. Repeated disables do
not nest; the next enable resumes formatting. These comments affect `ryl format`
only. `ryl check` still reports lint errors, and `ryl check --fix` can still fix
those errors. A formatter enable never re-enables a rule disabled by a rule directive.

Only own-line YAML comments are recognized. Text inside strings or scalar bodies
is ordinary data. Protected lines retain their spacing, quotes and line endings.
Structural conversions and re-indentation can be left as written when suppression
prevents applying them safely.

With `sequence-style = "block"`, the following is both input and output:

```yaml
# ryl format-disable
manually:    [1,2]
# ryl format-enable
formatted:
  - 1
  - 2
```

`ryl format --check` exits `0` for that file: the protected flow sequence produces
neither an `unformatted` finding nor a refusal notice.

## Embedded Markdown

Directives work inside YAML embedded in Markdown (front matter and fenced
`yaml` blocks). A directive in a fenced block applies within that block; see
[YAML in Markdown](markdown.md).
