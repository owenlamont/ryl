# yaml-directive

## What this rule does

Requires a `%YAML` version directive before every document, so each document
declares which YAML version its plain scalars resolve under. Under
[`target-version = "1.2"`](../yaml-version.md#how-ryl-handles-yaml-versions)
it also reports a document that declares `%YAML 1.1` or `1.0`. With
`present = false` it forbids every `%YAML` directive instead.

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
    *should* process it with a warning. For files that Kubernetes, Helm,
    Docker Compose or another program built on go-yaml loads, set
    `present = false`, which removes the directive. Not every Go parser has
    the bug: goccy/go-yaml accepts the directive. PyYAML accepts it but ignores it. See
    [Which YAML version each reader follows](../yaml-version.md#which-yaml-version-each-reader-follows).

## Configuration

`yaml-directive` is configured **only in TOML** &mdash; `[lint.rules.yaml-directive]`
in `.ryl.toml`/`ryl.toml` or `[tool.ryl.lint.rules.yaml-directive]` in
`pyproject.toml`. YAML config (including `-d` data) rejects it.

```toml
target-version = "1.2"

[lint.rules.yaml-directive]
level = "error"
present = true
```

| Option | Default | Description |
| :--- | :--- | :--- |
| `present` | `true` | When `true`, require a `%YAML` directive before every document. When `false`, forbid it. |

With `present = true`, enabling it by name, in config or with
`--enable yaml-directive`, beside `document-start` with `present = false` is a
configuration error (exit `2`): a directive needs the `---` that setting forbids. When only `ALL` enables it, in
config (`ALL = "enable"`) or with `--enable ALL`, the explicit `present = false`
wins instead: ryl prints the warning below, skips `yaml-directive` and lints with
the other rules.

```text
warning: `yaml-directive` and `document-start` with `present = false` are incompatible. Ignoring `yaml-directive`.
```

With `present = false` on both rules, they run together.

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

With `target-version = "1.2"`, a document declaring an older version:

```console
$ cat legacy.yaml
%YAML 1.1
---
mode: 0755
enabled: yes
timeout: 1:30
...
%YAML 1.1
---
released: 2026-10-10
$ ryl check legacy.yaml
legacy.yaml
  1:1       error    version directive "%YAML 1.1" is older than target-version "1.2"  (yaml-directive)
  7:1       error    version directive "%YAML 1.1" is older than target-version "1.2"  (yaml-directive)
```

### :wrench: After `ryl check --fix`

```yaml
%YAML 1.2
---
mode: 0o755
enabled: true
timeout: 90
...
%YAML 1.1
---
released: 2026-10-10
```

The second document keeps its report: a date has no YAML 1.2 core spelling.

With `present = false`, still under `target-version = "1.2"`:

```console
$ cat deploy.yaml
%YAML 1.2
---
name: web
...
%YAML 1.1
---
mode: 0755
enabled: yes
$ ryl check deploy.yaml
deploy.yaml
  1:1       error    found forbidden version directive "%YAML 1.2"  (yaml-directive)
  5:1       error    found forbidden version directive "%YAML 1.1"  (yaml-directive)
```

### :wrench: After `ryl check --fix` (with `present = false`)

```yaml
---
name: web
...
---
mode: 0o755
enabled: true
```

## Automatic fixing

`ryl check --fix` stamps `%YAML 1.2`, the version ryl already reads an undeclared
document as, so no value changes for ryl. It adds `---` where the document has no
start marker, keeps a shebang or `#cloud-config` first line in place, and ends an
open previous document with `...`, since a directive may only follow one.

With `present = true`, the fix runs only under
[`target-version = "1.2"`](../yaml-version.md#how-ryl-handles-yaml-versions). Under
the default `"1.1"`, `ryl format` keeps quotes a YAML 1.1 reader needs in an
undeclared document; stamping `%YAML 1.2` would drop them, so the rule reports
without fixing. Beside `document-end` with `present = false`, the fix skips a
document that would need a `...` and the diagnostic stays.

For a document declaring `%YAML 1.1` or `1.0`, the fix migrates the whole
document to 1.2 at once: it respells every plain scalar whose 1.1 value 1.2
reads differently (`yes` to `true`, `0755` to `0o755`, `1:30` to `90`) and
writes `%YAML 1.2`. It skips a document holding a scalar with no 1.2 spelling,
such as a date, or one that PyYAML reads as a string and other YAML 1.1
readers do not, such as `08` or `1e5`. [Migrating a `%YAML 1.1` document to
1.2](../yaml-version.md#migrating-a-yaml-11-document-to-12) lists the
spellings. A migrated `0o755` is still reported by `octal-values` unless
`forbid-explicit-octal` is off.

With `present = false`, the fix removes a directive declaring `%YAML 1.2` or
later, which ryl reads an undeclared document as, and migrates a `%YAML 1.1`
document as above before removing its directive, under either target. Under
`"1.1"` it also skips a document needing `0o777`, which PyYAML reads as a
string. A directive keeps its report when a comment shares its line or its
migration is skipped. Any `---` left behind is for `document-start`.

The rule is suppressed in YAML embedded in Markdown, which is not a standalone
file.

## Related rules

- [`document-start`](document-start.md) &mdash; requires the `---` marker a
  directive needs.
- [`schema-dependent-values`](schema-dependent-values.md) &mdash; flags the plain
  scalars whose value depends on the version.
