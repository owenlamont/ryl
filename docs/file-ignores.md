# File ignores

## Excluding files and paths

[Per-line ignores](per-line-ignores.md) suppress rules on lines matching a
pattern. This page covers the coarser layer: skipping whole files, or switching
off chosen rules for a path.

Three mechanisms, narrowing from left to right:

| What | Scope | Config format |
| :--- | :--- | :--- |
| [`ignore`](#ignore) | The file is never linted at all | YAML and TOML |
| [`per-file-ignores`](#per-file-ignores) | Named rules, for paths matching a glob | TOML only |
| [Per-rule `ignore`](#per-rule-ignore) | One rule, for paths matching a glob | YAML and TOML |

## `ignore`

A top-level `ignore` lists paths ryl skips entirely. No rule runs against them
and they produce no diagnostics.

```toml
[rules.document-start]
present = true

ignore = """
vendor/**
generated/**
"""
```

The same key works in yamllint-compatible YAML config:

```yaml
rules:
  document-start:
    present: true
ignore: |
  vendor/**
  generated/**
```

`ignore-from-file` reads the same patterns from a file instead, so a project can
reuse its `.gitignore`:

<!-- ryl-config-check: skip -->
```toml
[rules.document-start]
present = true

ignore-from-file = ".gitignore"
```

### Glob semantics

Patterns are gitignore-style: `**` crosses directory boundaries, a bare `*.yaml`
matches at any depth, and a leading `!` negates an earlier pattern.

```toml
[rules.document-start]
present = true

ignore = """
*.generated.yaml
!schema.generated.yaml
"""
```

### `ignore` also excludes files named on the command line

Unlike a shell glob, `ignore` is not just a directory-walk filter: a path
matching it is skipped even when passed explicitly, so `ryl check vendor/a.yaml`
reports nothing if `vendor/**` is ignored. This is the equivalent of ruff's
`force-exclude`, always on. See [YAML in Markdown](markdown.md) for how it
interacts with a pre-commit hook that passes filenames.

## `per-file-ignores`

`per-file-ignores` keeps linting a file but switches off the rules named for it.
Use it where a file legitimately breaks one rule and should still be checked for
everything else &mdash; a Helm values file with no document start, a workflow
file whose `on:` key trips [`truthy`](rules/truthy.md).

```toml
[rules.document-start]
present = true

[rules.truthy]

[per-file-ignores]
"**/values.yaml" = ["document-start"]
".github/workflows/*" = ["truthy"]
```

Each key is a path glob with the same semantics as `ignore`, including `!`
negation. Each value is a list of rule IDs.

`per-file-ignores` is **ryl-only** and configured in TOML only; yamllint has no
equivalent. Unlike [`per-line-ignores`](per-line-ignores.md), it accepts rule IDs
only &mdash; there is no `"ALL"` shorthand for every rule. To skip a file
outright, use `ignore`.

## Per-rule `ignore`

Every rule accepts its own `ignore`, scoping that one rule to a subset of the
tree. Other rules still run against the excluded paths.

```toml
[rules.line-length]
max = 80
ignore = """
docs/**
"""

[rules.colons]
```

Here `docs/**` is exempt from `line-length` but still checked by `colons`.

## Choosing between them

| You want to | Use |
| :--- | :--- |
| Never see the file again | `ignore` |
| Keep checking the file, minus one or two rules | `per-file-ignores` |
| Relax a single rule across a subtree | Per-rule `ignore` |
| Suppress a rule on matching *lines* rather than files | [`per-line-ignores`](per-line-ignores.md) |

## Related

- [Per-line ignores](per-line-ignores.md) &mdash; suppress rules on lines
  matching a regex or path.
- [Inline directives](directives.md) &mdash; in-file suppression with
  `# ryl disable` / `disable-line`.
- [Migrating from yamllint](getting-started/migrating-from-yamllint.md) &mdash;
  how `ignore` and `ignore-from-file` resolve relative paths per config source.
