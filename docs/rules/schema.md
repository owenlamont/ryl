# schema

In `ryl server`, clients supporting dynamic file watching register exact local schema
root and `$ref` paths, including missing files and arbitrary extensions. Creating,
saving or deleting a dependency revalidates open documents and updates push diagnostics
or requests a pull refresh. Local-only configurations require no network worker.
Unsaved schema buffers are not read; clients without dynamic registration must restart
the server or reopen documents after schema edits.

Hovering a key or value shows the prepared schema's `title` and `description`, even
when the value is invalid. Keys use their value's schema. Schema text is escaped plain
text, limited to 4,096 characters per annotation. Hover follows explicit `properties`,
homogeneous `items` and static `$ref` paths, with a 128-step limit; branches, tuple items,
dynamic references and documents containing aliases or merge keys receive no schema
hover. Direct properties beside unrelated branches remain available.
Hover never reads schema files, fetches resources or starts preparation. Pending or
unapproved schemas retain diagnostic explanations only. Files without associations
require no schema projection. Markdown regions use the host file's association.

## What this rule does

Validates YAML documents against user-supplied JSON Schemas. Schemas are supplied by the
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
| `path` | Relative schema path, anchored at the config base directory. A config under `.config/` uses the parent directory. Exactly one of `path` or `url` is required. |
| `url` | HTTPS schema URL on port 443. The user must independently grant the origin. |
| `pins` | Optional map of full resource URLs to `sha256:<64 hex>` content pins. |
| `files` | Optional path globs with `!` exclusions, like `key-ordering.orders`. Empty or omitted matches every file. |
| Association order | The first matching entry wins. Unmatched files and an enabled rule without associations are no-ops. |

`--stdin-filename` supplies the matching path for stdin. Anonymous stdin matches
only associations with empty `files`. Markdown regions match the host filename.
Normal rule directives, per-file ignores and per-line ignores apply.

## Loading and limits

Schemas compile during config resolution. Missing files, invalid schemas, unsupported
regex patterns, denied remote references and confinement failures are config errors (exit 2).
Both the root schema and referenced files must stay within the config base directory,
including after resolving symlinks. Absolute root paths and UNC references are refused.
Local roots retain confined `file:` references. Remote roots cannot read files.
Schema paths may contain spaces, brackets and other filename characters; `$ref`
URIs must percent-encode characters outside the URI path syntax.

Schema contents and local references contribute to the lint cache key. Schemas and
references share a 16 MiB byte limit and a 1,024-resource limit per config, bounding
what a large or hostile schema can cost. The limits are fixed.
Distinct URI aliases count separately toward both limits. Regex matching
uses a 100,000-step backtracking limit and 1 MiB compiled-pattern and DFA-cache limits. The draft
comes from `$schema`, defaulting to 2020-12.

## Remote schemas

Remote associations require a default-on `schema-network` build. A project requests
resources; grants come only from the launcher or user allowlist, including offline.
Every retrieved reference needs a grant for the resolved origin, even after `$id` rebasing.
`ryl server` prepares remote schemas in the background. Before accessing an ungranted
origin, the server reports an approval diagnostic on the affected file and configuration.
Use **Always allow `<origin>`** or **Allow once `<origin>`** from the diagnostic's quick
fixes. Allow once lasts for the server session; Always allow atomically updates the user
allowlist, refusing symlinks and non-regular files. A failed save reports a configuration
error and still grants access for the session. Workspace settings and project
configuration cannot grant access.

`server --offline` and `RYL_OFFLINE=1` disable approval actions and all DNS/HTTP access;
granted origins can still use cached schemas. Formatting never retrieves schemas.

<!-- ryl-config-check: skip -->
```toml
[[lint.rules.schema.schemas]]
url = "https://schemas.example.org/app.json"
files = ["app.yaml"]

[lint.rules.schema.pins]
"https://schemas.example.org/app.json" = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

```console
ryl check --schema-allow-origin https://schemas.example.org app.yaml
```

| User control | Behaviour |
| :--- | :--- |
| `--schema-allow-origin ORIGIN` | Repeatable exact HTTPS origin; no wildcard, path, query or fragment. |
| `RYL_SCHEMA_ALLOWED_ORIGINS` | Comma-separated origins, unioned with CLI grants. |
| User allowlist | `<user-config-dir>/ryl/allowed-schema-origins`, one origin per line, unioned with grants. |
| `--offline` / `RYL_OFFLINE=1` | No DNS or HTTP; cached full reference closure required. Stale entries are allowed. |
| `--no-cache` / `RYL_NO_CACHE=1` | Disable both lint and schema caches. Offline then cannot retrieve remote schemas. |
| `RYL_SCHEMA_CACHE_DIR` | Override the user schema cache (`<user-cache-dir>/ryl/schemas/v1`). |
| `--no-progress` | Suppress schema download messages on stderr. |

Only public destinations are accepted. Redirects, proxies, userinfo, encoded response
bodies and private/reserved IP addresses are refused. DNS checks the complete answer
before connecting, including mixed public/private answers. TLS uses bundled webpki roots.
Requests have a 5-second connect limit and a 30-second total limit; each process gets
64 requests and 60 seconds of network time. Query values are redacted in output.

Schema cache keys hash the complete canonical URL. Unpinned entries are fresh for
24 hours; stale entries refresh unconditionally. Pins are checked on every read and
permit indefinite reuse. Offline and pinned reads preserve fetch timestamps. Corrupt
entries are misses; symlinks are refused. Writes evict oldest entries above 256 MiB.

At load time, schemas with expanded weight above 1,000,000 are refused as
`schema too complex` (exit 2). The weight counts subschemas, declarations and
resolved references. Recursion without instance descent or with overlapping recursive
branches is also refused; distinct literal properties and disjoint object/array
paths are accepted. Repeated recursive tuple items are conservatively refused.
Drafts 04, 06 and 07 ignore keywords beside `$ref` during validation and complexity
screening. Unreferenced definitions are conservatively screened in newer drafts.
Draft 2019-09 `$recursiveRef` follows the validator's resource-root lookup even
when the reference contains a fragment. Dynamic references include all matching anchors as possible targets; this conservative
screen can refuse schemas whose runtime scopes would select fewer targets. Only
applicators and references active in the schema draft contribute to the screen.
More than 1,000,000 possible dynamic-reference edges are refused before expansion.

Projection is bounded to 1,000,000 nodes, 16 MiB of scalar and pointer storage, and
128 levels. A file reports at most 100 validation diagnostics plus a truncation note.
Users with no schema associations do no schema loading or projection. `ryl format`
does not load lint schemas.

## YAML values and locations

- Plain scalars resolve under YAML 1.2 core; explicit `%YAML 1.1` uses the existing
  version-aware resolver. Unsupported 1.1 values, including timestamps, are refused.
- Explicit YAML 1.1 float tags parse decimal or sexagesimal values rather than
  implicitly resolving integers (`!!float 012` is 12.0; `!!int 012` is 10).
- Explicit core tags must match the scalar's type and spelling; invalid tagged
  values produce `cannot validate` findings. The non-specific `!` tag resolves
  sequences and mappings as ordinary collections.
- Integers in the combined i64/u64 range and finite floats are supported.
- Aliases expand within the projection budget. Findings blame the alias use site.
- Merge keys, including aliased merge keys, expand before validation; explicit keys override inherited keys, and
  earlier mappings in merge sequences win. Malformed merges are refused.
- Duplicate decoded keys, non-string or complex keys and custom tags produce
  `cannot validate` findings (exit 1), rather than misleading type errors.
- Every document in a stream is validated independently; an empty document is null.
- Unexpected properties and invalid property names blame the key. Missing properties
  blame the containing mapping's start. Additional and unevaluated items blame the
  first offending item. Other findings use the instance value's source location.
- `oneOf` and `anyOf` findings select the most relevant branch error using
  [Python jsonschema's `best_match` heuristic](https://github.com/python-jsonschema/jsonschema/blob/main/jsonschema/exceptions.py).
  Equally ranked branches retain the summary. Messages mask instance values.

Editor diagnostics use the same engine. Watched schema-file edits invalidate prepared
schemas and revalidate affected documents. Schema hover and completion are separate work.

Sources: [JSON Schema](https://json-schema.org/specification),
[YAML 1.2.2](https://yaml.org/spec/1.2.2/).
