# File ignores

## Excluding files and paths

[Per-line ignores](per-line-ignores.md) suppress rules on lines matching a
pattern. This page covers the coarser layer: skipping whole files, or switching
off chosen rules for a path.

Three mechanisms, narrowing from left to right:

| What | Scope | Config format |
| :--- | :--- | :--- |
| [`exclude`](#exclude) | The file is never linted or formatted | TOML (`ignore` in YAML) |
| [`per-file-ignores`](#per-file-ignores) | Named rules, for paths matching a glob | TOML only |
| [Per-rule `ignore`](#per-rule-ignore) | One rule, for paths matching a glob | YAML and TOML |

## `exclude`

A top-level `exclude` lists paths ryl skips entirely. No rule runs against them,
`ryl format` leaves them alone, and they produce no diagnostics.

```toml
exclude = """
vendor/**
generated/**
"""

[lint.rules.document-start]
present = true
```

`exclude` replaces the deprecated top-level `ignore`, which still works but warns
until `ryl --migrate-configs` renames it. yamllint-compatible YAML config keeps
yamllint's `ignore` key:

```yaml
rules:
  document-start:
    present: true
ignore: |
  vendor/**
  generated/**
```

`exclude-from-file` (`ignore-from-file` in YAML) reads the same patterns from a
file instead, so a project can reuse its `.gitignore`:

<!-- ryl-config-check: skip -->
```toml
exclude-from-file = ".gitignore"

[lint.rules.document-start]
present = true
```

### Glob semantics

Patterns are gitignore-style: `**` crosses directory boundaries, a bare `*.yaml`
matches at any depth, and a leading `!` negates an earlier pattern.

On Windows and macOS, path globs and config filename comparisons ignore ASCII
case. The policy applies to `exclude`, `[files]`, rule ignores, per-line path
filters and key-ordering file selectors, including stdin filenames and the language
server. Other platforms keep case-sensitive matching.

A pattern with a directory in it is anchored at the directory holding the config
file, whether discovered or passed with `-c`, or at the working directory for inline
`-d` config. A file outside that directory
matches by file name only, so `*.lock.yaml` still applies to it and
`.github/workflows/*` does not &mdash; the same as ruff.

```toml
exclude = """
*.generated.yaml
!schema.generated.yaml
"""

[lint.rules.document-start]
present = true
```

### `exclude` also applies to files named on the command line

Unlike a shell glob, `exclude` is not just a directory-walk filter: a path
matching it is skipped even when passed explicitly, so `ryl check vendor/a.yaml`
reports nothing if `vendor/**` is excluded. This is the equivalent of ruff's
`force-exclude`, always on. See [YAML in Markdown](markdown.md) for how it
interacts with a pre-commit hook that passes filenames.

## `per-file-ignores`

`per-file-ignores` keeps linting a file but switches off the rules named for it.
Use it where a file legitimately breaks one rule and should still be checked for
everything else &mdash; a Helm values file with no document start, a workflow
file whose `on:` key trips [`truthy`](rules/truthy.md).

```toml
[lint.rules.document-start]
present = true

[lint.rules.truthy]

[lint.per-file-ignores]
"**/values.yaml" = ["document-start"]
".github/workflows/*" = ["truthy"]
```

Each key is a path glob with the same semantics as `exclude`, including `!`
negation. Each value is a list of rule IDs, or `["ALL"]` to switch off every rule
for the matching files:

```toml
[lint.rules]
truthy = "enable"

[lint.per-file-ignores]
"**/pnpm-*.yaml" = ["ALL"]
```

A file under `["ALL"]` is still parsed, so a syntax error in it is still
reported and the run exits `1`. To skip the file entirely, use `exclude`.

`per-file-ignores` is **ryl-only** and configured in TOML only (yamllint has no
equivalent); it is rejected in yamllint-compatible YAML config.

## Per-rule `ignore`

Every rule accepts its own `ignore`, scoping that one rule to a subset of the
tree. Other rules still run against the excluded paths.

```toml
[lint.rules.line-length]
max = 80
ignore = """
docs/**
"""

[lint.rules.colons]
```

Here `docs/**` is exempt from `line-length` but still checked by `colons`.

## Choosing between them

| You want to | Use |
| :--- | :--- |
| Never see the file again | `exclude` |
| Keep checking the file, minus one or two rules | `per-file-ignores` |
| Check only that the file parses | `per-file-ignores` with `["ALL"]` |
| Relax a single rule across a subtree | Per-rule `ignore` |
| Suppress a rule on matching *lines* rather than files | [`per-line-ignores`](per-line-ignores.md) |

## Related

- [Per-line ignores](per-line-ignores.md) &mdash; suppress rules on lines
  matching a regex or path.
- [Inline directives](directives.md) &mdash; in-file suppression with
  `# ryl disable` / `disable-line`.
- [Migrating from yamllint](getting-started/migrating-from-yamllint.md) &mdash;
  how `exclude` and `exclude-from-file` resolve relative paths per config source.
