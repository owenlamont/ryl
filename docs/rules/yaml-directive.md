# yaml-directive

## What this rule does

Requires a `%YAML` version directive before every document, so each document
declares which YAML version its plain scalars resolve under.

## Why this matters

- **No guessing.** A document with no directive is YAML 1.2 to a 1.2 reader and
  YAML 1.1 to PyYAML, so `yes`, `22:22` or `2026-10-10` can load differently. A
  directive states the author's version once.
- **Per document.** A directive binds only the document it precedes (YAML 1.2.2
  §6.8.1), so every document in a stream needs its own.

!!! warning "Not for go-yaml consumers"

    go-yaml v2, v3 and v4, and `sigs.k8s.io/yaml`, reject any file containing
    `%YAML 1.2` (`found incompatible YAML document`). That is a conformance bug
    in those parsers, not in the file: YAML 1.2.2 §6.8.1 says a 1.2 processor
    *must* accept `%YAML 1.2`, and even YAML 1.1 §7.1.1 says a 1.1 processor
    *should* process it with a warning. Do not enable this rule for
    files that Kubernetes, Helm, Docker Compose or another program built on
    go-yaml loads. Not every Go parser has the bug: goccy/go-yaml accepts the
    directive. PyYAML accepts it but ignores it. See
    [What `%YAML 1.2` does for each reader](../yaml-version.md#what-yaml-12-does-for-each-reader).

## Configuration

`yaml-directive` is configured **only in TOML** &mdash; `[lint.rules.yaml-directive]`
in `.ryl.toml`/`ryl.toml` or `[tool.ryl.lint.rules.yaml-directive]` in
`pyproject.toml`. YAML config (including `-d` data) rejects it. It has no options.

```toml
target-version = "1.2"

[lint.rules.yaml-directive]
level = "error"
```

Enabling it beside `document-start` with `present = false` is a configuration
error (exit `2`): a directive needs the `---` that setting forbids.

## Examples

```console
$ cat config.yaml
name: web
---
name: db
$ ryl check config.yaml
config.yaml
  1:1       error    missing version directive "%YAML"  (yaml-directive)
  2:1       error    missing version directive "%YAML"  (yaml-directive)
```

### :wrench: After `ryl check --fix` (with `target-version = "1.2"`)

```yaml
%YAML 1.2
---
name: web
...
%YAML 1.2
---
name: db
```

## Automatic fixing

`ryl check --fix` stamps `%YAML 1.2`, the version ryl already reads an undeclared
document as, so no value changes for ryl. It adds `---` where the document has no
start marker, keeps a shebang or `#cloud-config` first line in place, and ends an
open previous document with `...`, since a directive may only follow one.

The fix runs only under
[`target-version = "1.2"`](../yaml-version.md#how-ryl-handles-yaml-versions). Under
the default `"1.1"`, `ryl format` keeps quotes a YAML 1.1 reader needs in an
undeclared document; stamping `%YAML 1.2` would drop them, so the rule reports
without fixing. Beside `document-end` with `present = false`, the fix skips a
document that would need a `...` and the diagnostic stays.

The rule is suppressed in YAML embedded in Markdown, which is not a standalone
file.

## Related rules

- [`document-start`](document-start.md) &mdash; requires the `---` marker a
  directive needs.
- [`schema-dependent-values`](schema-dependent-values.md) &mdash; flags the plain
  scalars whose value depends on the version.
