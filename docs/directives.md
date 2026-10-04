# Inline directives

## Disabling rules from within a file

Sometimes a single line legitimately breaks a rule and changing the file is the
wrong fix. ryl supports inline comment directives that switch rules off for part
of a file, just like yamllint. The preferred spelling uses `ryl`:

```yaml
key:   value  # ryl disable-line rule:colons
```

Every directive is an ordinary YAML comment, so it never changes how the
document parses.

## Forms

There are two scopes &mdash; a single line, or a block that runs until it is
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

It must be the first line, with no `rule:` tokens. For yamllint parity the `#`
may be followed by any spacing (`#ryl disable-file` is accepted too).

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

For drop-in compatibility with projects migrating from yamllint, the
`# yamllint …` spelling is accepted as an alias everywhere `# ryl …` is:

```yaml
key:   value  # yamllint disable-line rule:colons
```

Both spellings follow yamllint's exact grammar. A comment (or, for
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

Syntax errors are always reported; no directive can suppress them.

## Interaction with `--fix`

`--fix` honours directives too: a fixer never rewrites a line whose rule is
disabled. Running `ryl check --fix` over the block above leaves `a:   1` and `b:   2`
untouched while still fixing `c:   3`.

## Embedded Markdown

Directives work inside YAML embedded in Markdown (front matter and fenced
`yaml` blocks). A directive in a fenced block applies within that block; see
[YAML in Markdown](markdown.md).
