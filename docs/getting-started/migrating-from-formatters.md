# Migrating from YAML formatters

Import a static Prettier, yamlfmt, dprint pretty_yaml, or yamlfix configuration:

```bash
# Preview generated TOML without changing files
ryl --migrate-configs --migrate-stdout

# Select a formatter when several configurations coexist
ryl --migrate-configs --migrate-from prettier --migrate-write

# Import one explicitly named configuration
ryl --migrate-configs --migrate-from yamlfix --migrate-root config/yamlfix.toml --migrate-write
```

`--migrate-from` accepts `yamllint`, `prettier`, `yamlfmt`, `dprint`, and `yamlfix`.
The `pretty-yaml` alias selects dprint. Selection affects project migration;
user-global migration remains a yamllint conversion.

The importer targets the latest audited releases: Prettier 3.9.10,
yamlfmt 0.21.0, dprint 0.64.3 with pretty_yaml 0.6.0, and yamlfix 1.19.1.
Older configurations may work, but defaults and option interpretation follow
those releases. Review the preview before replacing a formatter.

## Sources and precedence

| Formatter | Static sources |
| :--- | :--- |
| Prettier | `package.json` or `package.yaml` with `prettier`, `.prettierrc`, JSON, YAML, JSON5, and TOML configurations |
| yamlfmt | `.yamlfmt`, `yamlfmt.yaml`, `yamlfmt.yml`, `.yamlfmt.yaml`, `.yamlfmt.yml`; the basic formatter only |
| dprint | `dprint.json`, `dprint.jsonc`, `.dprint.json`, `.dprint.jsonc`, in that order; a pretty_yaml plugin URL, `npm:dprint-plugin-yaml` reference, or `yaml` section identifies YAML settings |
| yamlfix | The nearest ancestor of each filename: `pyproject.toml` with `[tool.yamlfix]`, `yamlfix.toml`, `.yamlfix.toml`; later filenames override earlier settings |

Prettier follows its configuration filename priority. JavaScript and TypeScript
configurations and shared configuration packages are identified but never
executed. A higher-priority unsupported configuration prevents importing a
lower-priority configuration with different effective settings.
Shared dprint configurations and plugins are never fetched or executed.
EditorConfig, environment variables, command-line overrides, file-selection
patterns, and per-file overrides are not applied; migration warns about known
sources of differences.

Multiple formatter families in one directory require `--migrate-from`.
Multiple yamlfmt filenames require selecting one file with `--migrate-root`,
because yamlfmt does not give those filenames deterministic precedence.
An explicit yamlfix file imports that file alone. Standalone yamlfix TOML files
use bare keys, rather than a `[yamlfix]` section.

A colocated yamllint configuration contributes lint settings to the same
`.ryl.toml`. Explicit formatter-file imports also preserve colocated lint settings;
cleanup retains those contributing lint files outside the selected file scope. Formatter width and indentation use top-level formatting settings;
the original lint thresholds remain independent. Existing ryl-native
configuration blocks migration writes. A formatter-only child configuration is also skipped
when a new target would hide inherited yamllint rules; migration retains the source
and warns you to combine lint and formatter settings manually. Independent directories
and nested formatter-only configurations can still migrate.
Preview, atomic writes, source symlink refusal,
and backup collision checks follow the
[yamllint migration safeguards](migrating-from-yamllint.md#automatic-migration).

`--migrate-stdout` sends progress to stderr and prints only TOML on stdout, so a
single-target export can be redirected into a configuration file. Existing native
targets remain untouched: the export includes a comment saying the candidate was
not merged and must be merged by hand. With multiple targets, each has a comment
header; the concatenated sections are separate configurations, not one config.

Migration exits `2` when a source is wholly skipped or a write is refused, including
an existing native target, executable-only config, or unresolved inheritance.
Approximations and retained comments produce warnings and exit `0`; `--strict`
makes warnings exit `2`. A clean rerun exits `0`.

## Effective settings and gaps

Migration writes representable source defaults as well as explicit options.
yamlfmt imports follow its numeric coercions and effective encoder defaults,
including legacy octal numbers and fractional integer truncation.
Generated `[format]` settings include quote presentation, prose wrapping,
line endings, collection policies, comments, blank lines, and document markers
where supported. Formatter-only imports enable no lint rules.

YAML-only Prettier overrides are flattened into the imported settings in source
order; later overrides win. Supported globs have a literal `.yaml` or `.yml`
extension, or a terminal brace list containing only those extensions, such as
`**/*.{yaml,yml}`. The glob prefix may use ordinary wildcards and character classes,
but not negation, escaping, braces, or extglobs. Every `files` glob must qualify,
and `excludeFiles` must be absent. Flattening applies the selected options to all
YAML files, including when a source glob selected only one directory.

Migration imports `.prettierignore` as gitignore patterns, including negations;
invalid lines produce individual warnings. It also imports dprint `excludes` and
yamlfmt `exclude` with `match_type: doublestar`. Yamlfmt patterns are anchored to
the config directory, cleaning `./`, internal `..`, and trailing separators;
absolute patterns inside that directory become relative. Paths outside that
directory and bare directory exclusions warn instead of being imported.
Yamlfmt matches paths relative to the invocation directory, with absolute patterns
matching only absolute inputs; migration assumes the config directory as the root.
Standard matching,
`regex_exclude`, and source `include` settings still warn.

Local dprint `extends` paths resolve relative to the extending file. Earlier bases
take precedence over later bases, and the extending config wins over its bases;
exclusions accumulate. Cycles, excessive inheritance, URLs, and unresolved paths
skip that source with a hint to flatten the configuration locally. Migration never
fetches shared configurations.

Imports report the same lint/format conflict notices as `ryl format`, using the
effective colocated lint configuration. Imported settings stay unchanged.

Unsupported settings produce warnings on stderr and comments in generated TOML.
Migration preserves YAML values and properties rather than adopting destructive
source behavior. Migration also keeps ryl's default
[YAML 1.1 reader compatibility](../yaml-version.md); a source formatter does not
establish which YAML readers consume your files. Config migration leaves source
ignore comments unchanged unless you opt into [comment conversion](#suppression-comments).

| Gap | Migration behavior |
| :--- | :--- |
| Forced quoting of plain strings | Retains existing quote presentation; warns when forcing was requested |
| Other per-file overrides or shared configurations | Warns about unsupported overrides; follows local dprint inheritance, but never fetches remote configurations |
| Source-specific flow wrapping, bracket padding, or trailing commas | Uses supported ryl layout settings; warns about differences |
| yamlfmt YAML merge keys | Retains the source without importing formatter settings; expand merged settings manually and rerun |
| yamlfmt scalar spellings requiring unsupported resolution | Retains the source without importing formatter settings; quote string values, use plain decimal numbers, or configure ryl manually |
| Removing document-start markers | Preserves existing markers; warns |
| yamlfix null spelling and per-style prose behavior | Preserves null spellings and uses split-only line folding; warns |
| Dimensions outside ryl's supported range | Uses a supported fallback and warns |

## Suppression comments

Preview comment conversion separately, including when a native ryl config already exists:

```bash
ryl --migrate-directives --migrate-from prettier --migrate-root .
ryl --migrate-directives --migrate-from prettier --migrate-root . --migrate-write

# Combine config import and comment conversion
ryl --migrate-configs --migrate-directives --migrate-from yamlfmt --migrate-write
```

Without `--migrate-write`, source changes appear as unified diffs. `--migrate-root`
can select one application file or a directory; selecting a config file does not
implicitly scan its parent. Static source configuration is discovered independently
of existing ryl settings. Competing formatter configurations require `--migrate-from`;
use the same flag when no source config identifies the formatter.

| Source comment | Conversion |
| :--- | :--- |
| Active `# yamllint disable`, `enable`, `disable-line`, first-line `disable-file` | Equivalent `# ryl` spelling; rule selection and targets stay unchanged |
| `# prettier-ignore` | `# ryl format-ignore` for verified node attachments |
| pretty_yaml's configured ignore marker | `# ryl format-ignore` for verified node attachments |
| First-line `# !yamlfmt!:ignore` | `# ryl format-disable` for one document without an explicit end marker |

Only actual YAML comments change, including supported Markdown YAML regions. Scalar
contents and unrelated bytes remain unchanged. Formatter conversions preserve formatting
only; lint and safe lint fixes remain active. No additional runtime aliases are installed.

Uncertain attachments remain source comments with warnings: inline markers, reason
suffixes, properties, explicit keys, flow members, document crossings and existing native
formatter directives. Markers shared with lint directives are retained to preserve lint scope. Source per-file, plugin or pragma settings can also prevent proving
an equivalent scope. Prettier Markdown conversion supports front matter and lowercase
`yaml` fences when embedded formatting is enabled and the inferred or configured parser
supports the filename. Quarto and R Markdown need an explicit static Markdown parser;
other formatter embedding stays unchanged.
Dprint conversion requires the audited official pretty_yaml plugin; unknown plugin IDs,
remote inheritance and custom file associations remain unconverted.
Official pretty_yaml `0.5.x` and `0.6.0` references are supported, including valid
checksum suffixes on official WASM URLs.
Shared comment segments that ryl's legacy grammar does not recognize remain unchanged.

YAML-only Prettier overrides without parser, plugin, or pragma options permit YAML
comment conversion. Overrides provably limited to other literal extensions, such
as `*.md`, do not block YAML conversion. Other overrides retain source markers.
Every directive run reports converted and retained comment totals plus skipped
file counts; reruns still report unsupported retained markers.

Writes preserve supported encodings and BOMs. Files whose original bytes cannot round-trip
through their encoding are retained. Some encodings, line endings or filenames cannot produce an
applicable text diff; previews report the limitation. Discovered source configs and explicit config-import sources are excluded from
application writes. Symlinks are left unchanged. Combined migration cleans up eligible source configs only
after application writes succeed.

## Source cleanup

`--migrate-delete-old` and `--migrate-rename-old` retain all Prettier and dprint
configurations, `package.json`, `package.yaml`, and `pyproject.toml`, because
those sources can configure other languages or tools. Only recognized standalone
yamlfmt and yamlfix filenames with understood settings are eligible for cleanup.
Custom filenames, sources with unknown settings, skipped sources, and inherited
sources outside the migration root remain untouched, with a warning.
