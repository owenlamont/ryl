# YAML version compatibility

## How ryl handles YAML versions

ryl parses YAML with its own fork of [granit-parser][granit]. It resolves a
document with no `%YAML` directive under the YAML 1.2 core schema, as the spec
directs. By default (`target-version = "1.1"`), `ryl format` and `--fix` write
output that YAML 1.1 readers such as [PyYAML][pyyaml] and go-yaml load to the
same values, and the value rules flag plain scalars whose meaning differs
between the two versions, so files drift toward spellings both read alike. Set
`target-version = "1.2"` once every consumer is a YAML 1.2 reader.

```toml
target-version = "1.2"
```

`target-version` is a top-level TOML setting (`[tool.ryl]` in
`pyproject.toml`); YAML configs cannot set it. It names the oldest YAML version
whose readers must load ryl's output to the same values, as ruff's
`target-version` names the oldest Python the code must run on. Under `"1.2"`:

- `quoted-strings` and `ryl format` drop quotes only a 1.1 reader needs
  (`'no'` becomes `no`, `'1_000'` becomes `1_000`).
- `truthy` flags only the 1.2 booleans (`True`, `FALSE`), not `yes` or `on`.
- [`schema-dependent-values`](rules/schema-dependent-values.md) goes quiet.
- [`yaml-directive`](rules/yaml-directive.md) offers its `%YAML 1.2` fix, and
  reports a `%YAML 1.1` document, whose fix migrates it to 1.2.

Some readers called YAML 1.2 still resolve 1.1 types: ruamel.yaml's default
loader and go-yaml v3 read `1_000`, `0b1010` and `2026-10-10` as non-strings.
Keep the default if such a reader loads your files.

  [granit]: https://github.com/bourumir-wyngs/granit-parser
  [pyyaml]: https://pyyaml.org/

## Respecting the `%YAML` version directive

The `%YAML` directive is part of the YAML specification (1.2.2 §6.8.1), so
ryl honours it per document, and it overrides `target-version`:

| Directive | ryl behaviour |
| :--- | :--- |
| none | Resolved as YAML 1.2; output also kept safe for 1.1 readers unless `target-version = "1.2"`. |
| `%YAML 1.2` | Resolved as YAML 1.2, for 1.2 readers only. |
| `%YAML 1.1` (or `1.0`) | Resolved as YAML 1.1, so scalars that 1.1 reads as a non-string keep that meaning. |
| `%YAML 1.3`+ (higher minor) | Processed as YAML 1.2 with a warning, as the spec directs. |
| `%YAML 2.0`+ (higher major) | Rejected as an error; the spec requires rejecting a higher major version. |
| two `%YAML` directives in one document | Rejected as an error, even when both name the same version. |

Under an explicit `%YAML 1.1` the value-resolving rules follow 1.1:
`quoted-strings` keeps quotes that are load-bearing under 1.1 and `truthy`
flags the 1.1 boolean words. `--fix` only makes edits that preserve the value
under the document's *declared* version. The `truthy` fix only re-cases
`True`/`FALSE` and the like, which are the same boolean under both versions.

### Migrating a `%YAML 1.1` document to 1.2

Under `target-version = "1.2"`, [`yaml-directive`](rules/yaml-directive.md)
reports each document that declares `%YAML 1.1` (or `1.0`), and `--fix`
migrates it in one edit: every plain scalar whose 1.1 value 1.2 reads
differently gets a 1.2 spelling of the same value, keys included, and the
directive becomes `%YAML 1.2`.

| `%YAML 1.1` | `%YAML 1.2` |
| :--- | :--- |
| `yes`, `No`, `on`, `OFF` | `true`, `false` |
| `0777`, `-0777` | `0o777`, `-511` |
| `22:22`, `1:30.5` | `1342`, `90.5` |
| `1_000`, `0b1010`, `-0x1F` | `1000`, `10`, `-31` |
| `!!bool yes` | `!!bool true` |

The scalars and the directive change together because neither half is safe
alone: `0o777` is a string to PyYAML and ruamel.yaml's 1.1 mode, so respelling
it before the directive changes would move its value.

The fix leaves a document alone, and its report stands, when a scalar has no
1.2 core spelling (a timestamp such as `2026-10-10`), or when PyYAML reads it
as a string but go-yaml or ruamel.yaml's 1.1 mode reads a number or boolean
(`08`, `1e5`, `-.5`, `y`, `0o777`). A scalar that would need respelling behind
the non-specific `!` tag (`! 0777`) blocks it too: the spec reads it as a
string, PyYAML and ruamel.yaml as if untagged. Rewrite or quote those by hand.
A file holding a raw NEL, LS or PS character is never migrated, since YAML 1.1
reads one as a line break and 1.2 as content; see
[`unicode-line-breaks`](rules/unicode-line-breaks.md). Every
other value keeps the meaning PyYAML and ruamel.yaml's 1.1 mode give it;
go-yaml, which reads `22:22` as a string, rejects a `%YAML 1.2` file anyway. A
merge key `<<` stays as it is.

Unless a document declares `%YAML 1.2` or `target-version = "1.2"` is set,
`quoted-strings` and `ryl format` keep the quotes on any scalar YAML 1.1 reads
as a non-string (`'no'`, `'on'`, `'yes'`, `'1_000'`), so the output means the
same to a 1.1 consumer such as PyYAML. The set is PyYAML's 1.1 resolver plus
`0o` octal, `y`, `Y`, `n` and `N`, which go-yaml v2 (used by Kubernetes) loads
as booleans, and go-yaml's float spellings (`'08'`, `'1e5'`, `'-.5'`, `'0_8'`),
which PyYAML reads as strings.

### What `%YAML 1.2` does for each reader

Declaring `%YAML 1.2` does not make every reader follow 1.2:

| Reader | `%YAML 1.2` |
| :--- | :--- |
| PyYAML | Ignored: still reads `yes` as `True`. |
| ruamel.yaml, goccy/go-yaml | Accepted. |
| go-yaml v2, v3 and v4, `sigs.k8s.io/yaml` | The whole file is rejected (`found incompatible YAML document`), so Kubernetes, Helm and Docker Compose cannot load it. |

The go-yaml rejection breaks both specs. YAML 1.2.2
[§6.8.1](https://yaml.org/spec/1.2.2/#681-yaml-directives) says a 1.2 processor
*must* accept `%YAML 1.2`, and YAML 1.1
[§7.1.1](https://yaml.org/spec/1.1/#id895631) says a 1.1 processor *should*
process a higher minor version with a warning. go-yaml accepts exactly `1.1` and
rejects every other version, although its README claims most of YAML 1.2.

## What is different in YAML 1.2

| Literal | YAML 1.1 | YAML 1.2 |
| :--- | :--- | :--- |
| `yes`, `no`, `on`, `off` and case variants (`Yes`, `No`, `ON`, ...) | Booleans | Plain strings |
| `y`, `Y`, `n`, `N` | Booleans | Plain strings |
| `true`, `True`, `TRUE`, `false`, `False`, `FALSE` | Booleans | Booleans |
| `null`, `Null`, `NULL`, `~` | Null | Null |
| `0755` (leading zero integer) | Octal | Decimal |
| `08` (leading zero, not octal) | String | Decimal |
| `22:22` (sexagesimal) | Integer 1342 | Plain string |
| `1_000`, `0b1010` | Integers | Plain strings |
| `2026-10-10` | Timestamp | Plain string |

ryl applies YAML 1.2 semantics to both linted files and configuration
files.

## Implications for the `quoted-strings` rule

With `quote-type: double, required: only-when-needed`, ryl considers
`"yes"` to need quoting because the bareword would parse as a boolean in
1.1. The safeguard applies unless the document declares `%YAML 1.2` or
`target-version = "1.2"` is set, since removing the quotes would change the
value for a 1.1 consumer.

## Implications for configuration files

ryl parses `.yamllint` / `.yamllint.yaml` / `.yamllint.yml` configs with
the same strict YAML 1.2 parser. A yamllint configuration that uses 1.1
booleans like:

<!-- ryl-config-check: skip -->
```yaml
rules:
  truthy:
    check-keys: no       # 1.1 boolean false
  empty-values:
    forbid-in-block-mappings: yes
```

…will fail to parse in ryl. Replace these values with their YAML 1.2
equivalents (`false` / `true`) or run the built-in converter to produce
a `.ryl.toml` instead:

```bash
ryl --migrate-configs --migrate-write
```

The migration converter writes TOML, where booleans have an unambiguous
syntax independent of YAML version.

## Implications for documents being linted

The rules below behave the way they do *because* ryl reads a directive-less
input as YAML 1.2 (an explicit `%YAML 1.1` shifts the value-resolving rules to
1.1, as above):

- [`truthy`](rules/truthy.md) &mdash; the rule itself still recognises
  the 1.1 truthy word list as ambiguous and flags barewords like `yes`
  or `On` so authors are warned about consumers that still use 1.1, unless
  `target-version = "1.2"` declares there are none.
- [`octal-values`](rules/octal-values.md) &mdash; `0755` is treated as
  decimal `755`; the rule can still report it for ambiguity reasons.
- [`float-values`](rules/float-values.md) &mdash; numeric formats follow
  the 1.2 spec.
- [`schema-dependent-values`](rules/schema-dependent-values.md) &mdash;
  flags the other plain scalars the two versions read differently, such as
  `2026-10-10`, `22:22` and `1_000`.
- [`yaml-directive`](rules/yaml-directive.md) &mdash; requires each document
  to declare its version, so no reader has to guess.

For documents that need to be portable between 1.1 and 1.2 consumers,
the safest pattern is to quote any bareword that 1.1 would coerce.
`quoted-strings` and `ryl format` keep those quotes for you.
