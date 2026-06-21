# YAML version compatibility

## ryl targets YAML 1.2 by default

ryl parses YAML with [granit-parser][granit] (a `saphyr-parser` fork). A
document with no version directive is resolved per the YAML 1.2 core schema,
matching the spec, which assumes a directive-less document conforms to 1.2.
yamllint, by comparison, is built on [PyYAML][pyyaml], which defaults to YAML
1.1 semantics for boolean and other implicit type resolution.

Most YAML files do not exercise the parts of the language where the two
versions disagree, so in practice the difference rarely shows up. The two
places it matters in ryl are:

- The [`quoted-strings`](rules/quoted-strings.md) rule when used with
  `required: "only-when-needed"`.
- Parsing of `.yamllint` / `.yamllint.yaml` / `.yamllint.yml`
  configuration files inherited from yamllint.

  [granit]: https://github.com/bourumir-wyngs/granit-parser
  [pyyaml]: https://pyyaml.org/

## Respecting the `%YAML` version directive

The `%YAML` directive is part of the YAML specification (1.2.2 §6.8.1), so
ryl honours it per document:

| Directive | ryl behaviour |
| :--- | :--- |
| none | Resolved as YAML 1.2 (the spec's default for a directive-less document). |
| `%YAML 1.2` | Resolved as YAML 1.2. |
| `%YAML 1.1` (or `1.0`) | Resolved as YAML 1.1, so scalars that 1.1 reads as a non-string keep that meaning. |
| `%YAML 1.3`+ (higher minor) | Processed as YAML 1.2 with a warning, as the spec directs. |
| `%YAML 2.0`+ (higher major) | Rejected as an error; the spec requires rejecting a higher major version. |
| two `%YAML` directives in one document | Rejected as an error, even when both name the same version. |

Under an explicit `%YAML 1.1` the value-resolving rules follow 1.1:
`quoted-strings` keeps quotes that are load-bearing under 1.1 and `truthy`
flags the 1.1 boolean words. ryl never rewrites a `%YAML 1.1` document to
1.2: `--fix` only makes edits that preserve the value under the document's
*declared* version, so it will not strip the quotes from `'no'` under
`%YAML 1.1`, where the bareword `no` is the boolean false.

## What is different in YAML 1.2

| Literal | YAML 1.1 | YAML 1.2 |
| :--- | :--- | :--- |
| `yes`, `no`, `on`, `off` and case variants (`Yes`, `No`, `ON`, ...) | Booleans | Plain strings |
| `true`, `True`, `TRUE`, `false`, `False`, `FALSE` | Booleans | Booleans |
| `null`, `Null`, `NULL`, `~` | Null | Null |
| `0755` (leading zero integer) | Octal | Decimal |

ryl applies YAML 1.2 semantics to both linted files and configuration
files.

## Implications for the `quoted-strings` rule

With `quote-type: double, required: only-when-needed`, yamllint considers
`"yes"` to need quoting (because the bareword would parse as a boolean in
1.1), so it accepts the quoted form. With no version directive ryl resolves
under 1.2, where `"yes"` is a string, so it reports the quotes as redundant.
Under an explicit `%YAML 1.1` ryl keeps the quotes, matching yamllint, since
removing them would change the value for a 1.1 consumer.

If your project still wants quotes around YAML 1.1 truthy words in
directive-less documents to protect consumers that use 1.1 parsers, either
declare `%YAML 1.1` in those documents or set:

```toml
[rules.quoted-strings]
required = true              # always quote, regardless of type
quote-type = "double"
```

…or rely on the [`truthy`](rules/truthy.md) rule to flag bareword
booleans instead and keep `quoted-strings` off.

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
1.1, as above). Most of these are intentional and align with yamllint when
yamllint is configured for 1.2 explicitly:

- [`truthy`](rules/truthy.md) &mdash; the rule itself still recognises
  the 1.1 truthy word list as ambiguous and flags barewords like `yes`
  or `On` so authors are warned about consumers that still use 1.1.
- [`octal-values`](rules/octal-values.md) &mdash; `0755` is treated as
  decimal `755`; the rule can still report it for ambiguity reasons.
- [`float-values`](rules/float-values.md) &mdash; numeric formats follow
  the 1.2 spec.

For documents that need to be portable between 1.1 and 1.2 consumers,
the safest pattern is to quote any bareword that 1.1 would coerce. ryl
will not get in your way as long as `quoted-strings.required` is `true`
(or unset / disabled) rather than `"only-when-needed"`, or the document
declares `%YAML 1.1` so ryl keeps those quotes for you.
