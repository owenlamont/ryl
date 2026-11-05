# YAML version compatibility

## How ryl handles YAML versions

A **reader** is the program or library loading your YAML. Two top-level TOML
settings distinguish who must read the output from what the input originally meant:

| Setting | Question it answers | Default | What it changes |
| :--- | :--- | :--- | :--- |
| `target-version` | Which readers must the output suit? | `"1.1"` | What the rules flag and what `ryl format` and `--fix` write |
| `source-version` | Which version did the author of an undeclared document mean? | `"1.2"` | Only the direction of an [unsafe fix](#unsafe-fixes-and-source-version) |

ryl parses YAML with its own fork of [granit-parser][granit]. A document with no
`%YAML` directive uses the YAML 1.2 **core schema**: the standard rules deciding
whether an unquoted value is a string, number, boolean or null. The default target
keeps output compatible with 1.1 readers too. See [what differs in 1.2](#what-is-different-in-yaml-12).

### Migrate undeclared YAML 1.1 files safely

For files written for [PyYAML][pyyaml-home] or another 1.1 reader:

1. Commit or back up the originals. Keep `target-version = "1.1"`, set
   `source-version = "1.1"`, and enable the migration rules:

    ```toml
    target-version = "1.1"
    source-version = "1.1"

    [lint.rules]
    truthy = "enable"
    octal-values = "enable"
    schema-dependent-values = "enable"
    ```

2. Preview with `ryl check --diff --unsafe-fixes .`; review the chosen meanings,
   then apply with `ryl check --fix --unsafe-fixes .`.
3. Resolve every remaining report manually, for example by quoting `zone: "08"`
   if the intended value is text. Some 1.1 readers disagree; check your own reader.
4. Load the original and edited files with your application's reader and compare
   values and types. Test the application with the intended 1.2 reader too.
5. Only once every consumer supports the intended 1.2 meanings, set
   `target-version = "1.2"` and remove `source-version`. The compatibility rules
   then go quiet; unresolved values such as `08` can change meaning without warning.

Do not apply the 1.1 source setting to GitHub Actions workflows: Actions treats `on:`
as text. Use a separate nearest config or the [workflow exemption](rules/truthy.md#github-actions-workflows).
Files declaring `%YAML 1.1` use the [directive migration](#migrating-a-yaml-11-document-to-12) instead.

A `%YAML` directive states a document's version outright, so no unsafe fix
applies to that document and `source-version` plays no part in it.

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
Keep the default if such a reader loads your files; see
[which YAML version each reader follows](#which-yaml-version-each-reader-follows).

  [granit]: https://github.com/bourumir-wyngs/granit-parser
  [pyyaml-home]: https://pyyaml.org/

## Respecting the `%YAML` version directive

The `%YAML` directive is part of the YAML specification (1.2.2 §6.8.1), so
ryl honours it per document, and it overrides `target-version`:

| Directive | ryl behaviour |
| :--- | :--- |
| none | Resolved as YAML 1.2; output also kept safe for 1.1 readers unless `target-version = "1.2"`. |
| `%YAML 1.2` | Resolved as YAML 1.2, for 1.2 readers only. |
| `%YAML 1.1` (or `1.0`) | Resolve scalar values as YAML 1.1; parse structure with the YAML 1.2 grammar. |
| `%YAML 1.3`+ (higher minor) | Processed as YAML 1.2 with a warning, as the spec directs. |
| `%YAML 2.0`+ (higher major) | Rejected as an error; the spec requires rejecting a higher major version. |
| two `%YAML` directives in one document | Rejected as an error, even when both name the same version. |

YAML 1.2 does not treat Unicode Next Line (NEL), Line Separator (LS), or Paragraph
Separator (PS) as line breaks. Using those characters as breaks is a syntax error,
even under `%YAML 1.1`; see [`unicode-line-breaks`](rules/unicode-line-breaks.md).

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
directive becomes `%YAML 1.2`. With the rule's `present = false`, the fix
removes the directive instead, under either target; under `"1.1"` it skips a
document needing `0o777`, which PyYAML reads as a string.

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

The fix leaves the whole document unchanged when the versions or readers cannot
agree on a replacement. Resolve the blocking input manually, then rerun:

| Input | Why migration stops | Action |
| :--- | :--- | :--- |
| Timestamp such as `2026-10-10` | No 1.2 core timestamp type | Choose text or an application-specific representation |
| `08`, `1e5`, `-.5`, `y`, `0o777` | 1.1 readers disagree on the type | Quote intended strings or write an unambiguous number/boolean |
| `! 0777` | The bare `!` tag requests a string in the spec, but some readers ignore it | Replace with an explicit string or number |
| Raw NEL, LS or PS | Line breaks in 1.1, content in 1.2 | Resolve the line breaks manually |

Other values keep PyYAML and ruamel.yaml's 1.1 meanings. go-yaml (used by Kubernetes
and Helm) reads `22:22` as text and rejects `%YAML 1.2` anyway. Merge keys `<<` stay unchanged.

Unless a document declares `%YAML 1.2` or `target-version = "1.2"` is set,
`quoted-strings` and `ryl format` keep the quotes on any scalar YAML 1.1 reads
as a non-string (`'no'`, `'on'`, `'yes'`, `'1_000'`), so the output means the
same to a 1.1 consumer such as PyYAML. The set is PyYAML's 1.1 resolver plus
`0o` octal, `y`, `Y`, `n` and `N`, which go-yaml v2 (used by Kubernetes) loads
as booleans, and go-yaml's float spellings (`'08'`, `'1e5'`, `'-.5'`, `'0_8'`),
which PyYAML reads as strings.

### Unsafe fixes and `source-version`

!!! warning "Choose the input's meaning before applying unsafe fixes"

    The default `source-version = "1.2"` assumes undeclared files were written for
    1.2: `yes` means text and `0777` means decimal 777. For files written for
    PyYAML, Ansible or another 1.1 reader, set `source-version = "1.1"` first.
    Otherwise an unsafe fix can change the values your application loads.

A document with no directive under the default `target-version = "1.1"` must
mean the same to both versions, but a scalar such as `yes` already does not: a
string to 1.2, a boolean to PyYAML. Rewriting it picks one reading, so
`truthy`, `octal-values` and `schema-dependent-values` offer an
[unsafe fix](rules.md#unsafe-fixes) that applies only under `--unsafe-fixes`,
`unsafe-fixes = true` or `lint.extend-safe-fixes`. The top-level
`source-version` says which reading the author meant, and the fix writes a
spelling both versions read that way:

| Scalar | `source-version = "1.2"` (default) | `source-version = "1.1"` |
| :--- | :--- | :--- |
| `yes`, `off` | `"yes"`, `"off"` | `true`, `false` |
| `0777`, `0o17` | `777`, `15` | `511`, left alone |
| `1_000`, `0b1010` | `"1_000"`, `"0b1010"` | `1000`, `10` |
| `22:22`, `1:30.5` | `"22:22"`, `"1:30.5"` | `1342`, `90.5` |
| `08` | `8` | left alone |
| `2026-10-10`, `y` | `"2026-10-10"`, `"y"` | left alone |

Under `"1.1"` the fix leaves a scalar alone where the migration above declines:
YAML 1.1 readers disagree on it (`08`, `0o17`, `y`), or YAML 1.2
has no type for it (a timestamp). Either way it leaves alone an integer past 64
bits, which go-yaml reads as a float. Quotes follow `quoted-strings`'
`quote-type = "single"` when that rule is on, and a boolean takes the first
spelling `truthy`'s `allowed-values` permits.

`source-version` steers only the fix: every rule, the formatter and the value
checks still read an undeclared document as YAML 1.2. Like any setting, the
nearest config wins, so a PyYAML-era subtree can carry its own
`source-version = "1.1"`.

For a plain `yes` with `truthy` enabled, target 1.1 plus source 1.2 offers `"yes"`;
target 1.1 plus source 1.1 offers `true`. Target 1.2 reports neither: `yes` is already
text, and `source-version` has no effect. Follow the [ordered migration recipe](#migrate-undeclared-yaml-11-files-safely)
before changing the target.

## What is different in YAML 1.2

| Literal | YAML 1.1 | YAML 1.2 |
| :--- | :--- | :--- |
| `yes`, `no`, `on`, `off` and case variants (`Yes`, `No`, `ON`, ...) | Booleans | Plain strings |
| `y`, `Y`, `n`, `N` | Booleans | Plain strings |
| `true`, `True`, `TRUE`, `false`, `False`, `FALSE` | Booleans | Booleans |
| `null`, `Null`, `NULL`, `~` | Null | Null |
| `0755` (leading zero integer) | Octal | Decimal |
| `08` (leading zero, not octal) | String | Decimal |
| `22:22` (base-60, or sexagesimal) | Integer 1342 | Plain string |
| `1_000`, `0b1010` | Integers | Plain strings |
| `2026-10-10` | Timestamp | Plain string |

ryl applies YAML 1.2 semantics to both linted files and configuration files.

## Which YAML version each reader follows

Few readers follow one YAML version throughout, and a `%YAML` directive does
not switch most of them. Pick `target-version` for the oldest reader that
loads your files. Each reader below loaded eight undeclared plain scalars
(`yes`, `on`, `0777`, `0o17`, `1_000`, `0b1010`, `22:22`, `2026-10-10`) into
a generic map or object, then the same file under `%YAML 1.2` and `%YAML 1.1`.
**1.1** or **1.2** means all eight took that version's value, and "no change"
means the directive changed no value. "Version option" names the loader
setting that changes which version's values the reader resolves, each run to
confirm the change. [In Defense of YAML][posit-yaml] covers
why the two versions differ.

Last verified: 2026-11-02.

| Reader | Version | Undeclared | `%YAML 1.2` | `%YAML 1.1` | Version option | Used by |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| [PyYAML][pyyaml] `safe_load` | 6.0.3 | 1.1 | No change | No change | No | [yamllint][yamllint], [Ansible][ansible] |
| [ruamel.yaml][ruamel], default and `typ='safe'` | 0.19.1 | 1.2, except `1_000`, `0b1010` and `2026-10-10` load as non-strings | No change | 1.1 | `yaml.version = (1, 1)`, or via `%YAML` | [pre-commit-hooks][pre-commit-hooks] |
| [py-yaml12][py-yaml12] | 0.2.0 | 1.2 | No change | No change | No | |
| [`gopkg.in/yaml.v2`][yaml-v2-old], [`go.yaml.in/yaml/v2`][yaml-v2] | 2.4.0, 2.4.4 | 1.1, except `0o17` is 15 and `22:22` and `2026-10-10` are strings | Rejected | No change | No | |
| [`sigs.k8s.io/yaml`][k8s-yaml] | 1.6.0 | As go-yaml v2, with numbers as `float64` | Rejected | No change | No | [Kubernetes][kubernetes], [Helm][helm] |
| [`gopkg.in/yaml.v3`][yaml-v3-old], [`go.yaml.in/yaml/v3`][yaml-v3] | 3.0.1, 3.0.6 | `yes`, `on` and `22:22` are strings, but `0777` is 511, `1_000` and `0b1010` integers and `2026-10-10` a time; into a `bool` field `yes` is `true` | Rejected | No change | No | |
| [`go.yaml.in/yaml/v4`][yaml-v4] | 4.0.0-rc.8 (pre-release) | As go-yaml v3 | Rejected | No change | No | [Docker Compose][compose] (pins rc.6) |
| [goccy/go-yaml][goccy] | 1.19.2 | 1.2, except `0777` is 511 and `1_000` and `0b1010` are integers | No change | No change | No | |
| [js-yaml][js-yaml] | 5.5.0 | 1.2 | No change | No change | `schema: YAML11_SCHEMA` | |
| [js-yaml][js-yaml] | 4.3.2 | 1.2, except `0b1010` is 10 and `2026-10-10` a `Date` | No change | No change | No | [ESLint][eslint] (`@eslint/eslintrc`) |
| [`yaml`][eemeli-yaml] (eemeli) | 2.9.1 | 1.2 | No change | 1.1 | `version: '1.1'`; a `%YAML` directive overrides it | [Prettier][prettier] |
| [Psych][psych] `safe_load` with `permitted_classes: [Date]` (Ruby 4.0.9); without it the date is an error | 5.3.1 | 1.1, except `22:22` is 80520 | No change | No change | No | |
| [SnakeYAML][snakeyaml] | 2.7 | 1.1 | No change | No change | No | [Spring Boot][spring-boot], [Jackson 2.x][jackson-yaml] |
| [SnakeYAML Engine][snakeyaml-engine] | 3.2 | Every scalar a string, `0777` and `0o17` included | No change | No change | `setSchema(new CoreSchema())` gives 1.2; nothing selects 1.1 | |
| [YamlDotNet][yamldotnet] | 18.1.0 | Every scalar a string; into a `bool` or `int` field `yes` is `true`, `0777` 511, `1_000` 1000 and `0b1010` 10 | No change | No change | `WithAttemptingUnquotedStringTypeDeserialization` reads `0777` as 777; nothing selects 1.1 | |
| [serde_yaml][serde-yaml] (deprecated) | 0.9.34 | 1.2, except `0777` is a string and `0b1010` is 10 | No change | No change | No | |
| [serde_yml][serde-yml] | 0.0.13 | 1.2 | No change | No change | No | |
| [saphyr][saphyr] | 0.3.3 | 1.2 | No change | No change | No | |

"Rejected" means the reader refuses the whole file
(`found incompatible YAML document`), so Kubernetes, Helm and Docker Compose
cannot load a file declaring `%YAML 1.2`. The go-yaml rejection breaks both specs.
YAML 1.2.2
[§6.8.1](https://yaml.org/spec/1.2.2/#681-yaml-directives) says a 1.2 processor
*must* accept `%YAML 1.2`, and YAML 1.1
[§7.1.1](https://yaml.org/spec/1.1/#id895631) says a 1.1 processor *should*
process a higher minor version with a warning. go-yaml accepts exactly `1.1` and
rejects every other version, although its README claims most of YAML 1.2.

  [posit-yaml]: https://opensource.posit.co/blog/2026-05-21_in-defense-of-yaml/
  [pyyaml]: https://github.com/yaml/pyyaml
  [ruamel]: https://yaml.dev/doc/ruamel.yaml/
  [py-yaml12]: https://github.com/posit-dev/py-yaml12
  [yaml-v2-old]: https://pkg.go.dev/gopkg.in/yaml.v2
  [yaml-v2]: https://pkg.go.dev/go.yaml.in/yaml/v2
  [k8s-yaml]: https://github.com/kubernetes-sigs/yaml
  [yaml-v3-old]: https://pkg.go.dev/gopkg.in/yaml.v3
  [yaml-v3]: https://pkg.go.dev/go.yaml.in/yaml/v3
  [yaml-v4]: https://pkg.go.dev/go.yaml.in/yaml/v4
  [goccy]: https://github.com/goccy/go-yaml
  [js-yaml]: https://github.com/nodeca/js-yaml
  [eemeli-yaml]: https://eemeli.org/yaml/
  [psych]: https://github.com/ruby/psych
  [snakeyaml]: https://bitbucket.org/snakeyaml/snakeyaml
  [snakeyaml-engine]: https://bitbucket.org/snakeyaml/snakeyaml-engine
  [yamldotnet]: https://github.com/aaubry/YamlDotNet
  [serde-yaml]: https://github.com/dtolnay/serde-yaml
  [serde-yml]: https://github.com/sebastienrousseau/serde_yml
  [saphyr]: https://github.com/saphyr-rs/saphyr
  [yamllint]: https://github.com/adrienverge/yamllint
  [ansible]: https://github.com/ansible/ansible
  [pre-commit-hooks]: https://github.com/pre-commit/pre-commit-hooks
  [kubernetes]: https://github.com/kubernetes/kubernetes
  [helm]: https://github.com/helm/helm
  [compose]: https://github.com/docker/compose
  [eslint]: https://github.com/eslint/eslint
  [prettier]: https://github.com/prettier/prettier
  [spring-boot]: https://github.com/spring-projects/spring-boot
  [jackson-yaml]: https://github.com/FasterXML/jackson-dataformats-text

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
  to declare its version. Avoid requiring `%YAML 1.2` for Kubernetes, Helm or
  Compose; see the [go-yaml warning](rules/yaml-directive.md).

For documents that need to be portable between 1.1 and 1.2 consumers,
the safest pattern is to quote any bareword that 1.1 would coerce.
`quoted-strings` and `ryl format` keep those quotes for you.
