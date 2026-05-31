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

## yamllint compatibility

For drop-in compatibility with projects migrating from yamllint, the
`# yamllint …` spelling is accepted as an alias everywhere `# ryl …` is:

```yaml
key:   value  # yamllint disable-line rule:colons
```

Both spellings follow yamllint's exact grammar. A comment is only treated as a
directive when it matches precisely &mdash; a single space after `#`, single
spaces between words, and `rule:` before each id. Near-misses are plain
comments and do **not** disable anything:

```yaml
a:   1  #   ryl disable-line rule:colons   # extra spaces → not a directive
a:   1  # ryl disable-line colons          # missing `rule:` → not a directive
```

Syntax errors are always reported; no directive can suppress them.

## Interaction with `--fix`

`--fix` honours directives too: a fixer never rewrites a line whose rule is
disabled. Running `ryl --fix` over the block above leaves `a:   1` and `b:   2`
untouched while still fixing `c:   3`.

## Embedded Markdown

Directives work inside YAML embedded in Markdown (front matter and fenced
`yaml` blocks). A directive in a fenced block applies within that block; see
[YAML in Markdown](markdown.md).
