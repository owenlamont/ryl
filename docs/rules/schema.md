# schema

## What this rule does

Validates YAML documents against local JSON Schemas. Schemas are supplied by the
project; ryl supplies no application-specific schema catalog. No fix is available.

## Configuration

TOML only, in `.ryl.toml`, `ryl.toml`, or `[tool.ryl.lint.rules.schema]` in
`pyproject.toml`. YAML configuration rejects `schema`.

<!-- ryl-config-check: skip -->
```toml
[lint.rules.schema]
level = "error"

[[lint.rules.schema.schemas]]
path = "schemas/github-workflow.json"
files = [".github/workflows/*.yml", ".github/workflows/*.yaml"]
```

| Setting | Meaning |
| :--- | :--- |
| `path` | Required relative schema path, anchored at the config base directory. A config under `.config/` uses the parent directory. |
| `files` | Optional path globs with `!` exclusions, like `key-ordering.orders`. Empty or omitted matches every file. |
| Association order | The first matching entry wins. Unmatched files and an enabled rule without associations are no-ops. |

`--stdin-filename` supplies the matching path for stdin. Anonymous stdin matches
only associations with empty `files`. Markdown regions match the host filename.
Normal rule directives, per-file ignores and per-line ignores apply.

## Loading and limits

Schemas compile during config resolution. Missing files, invalid schemas, unsupported
regex patterns, remote references and confinement failures are config errors (exit 2).
Both the root schema and referenced files must stay within the config base directory,
including after resolving symlinks. Absolute root paths and UNC references are refused.
Only local `file:` references are retrieved; network opt-in is not supported yet.

Schema contents and local references contribute to the lint cache key. Schemas and
references share a 16 MiB byte limit and a 1,024-resource limit per root schema.
Distinct URI aliases count separately toward both limits. Regex matching
uses a 100,000-step backtracking limit and 1 MiB compiled-pattern and DFA-cache limits. The draft
comes from `$schema`, defaulting to 2020-12.

Projection is bounded to 1,000,000 nodes, 16 MiB of scalar and pointer storage, and
128 levels. A file reports at most 100 validation diagnostics plus a truncation note.
Users with no schema associations do no schema loading or projection. `ryl format`
does not load lint schemas.

## YAML values and locations

- Plain scalars resolve under YAML 1.2 core; explicit `%YAML 1.1` uses the existing
  version-aware resolver. Unsupported 1.1 values, including timestamps, are refused.
- Integers in the combined i64/u64 range and finite floats are supported.
- Aliases expand within the projection budget. Findings blame the alias use site.
- Merge keys expand before validation; explicit keys override inherited keys, and
  earlier mappings in merge sequences win. Malformed merges are refused.
- Duplicate decoded keys, non-string or complex keys and custom tags produce
  `cannot validate` findings (exit 1), rather than misleading type errors.
- Every document in a stream is validated independently; an empty document is null.
- Unexpected properties blame the key. Missing properties blame the containing
  mapping's start. Other findings use the instance value's source location.

Editor diagnostics use the same engine. Schema-file edits take effect when the
configuration reloads; schema-file watching, hover and completion are separate work.

Sources: [JSON Schema](https://json-schema.org/specification),
[YAML 1.2.2](https://yaml.org/spec/1.2.2/).
