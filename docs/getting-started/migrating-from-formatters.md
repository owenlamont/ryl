# Migrating from YAML formatters

Import a Prettier, yamlfmt, dprint pretty_yaml, or yamlfix configuration stored as
data, without executing configuration code:

| Command | Result |
| :--- | :--- |
| `ryl --migrate-configs` | List the migration plan |
| Add `--migrate-stdout` | Print generated TOML; progress and warnings go to stderr |
| Add `--migrate-write` | Write the configurations |

```bash
# Preview generated TOML without changing files
ryl --migrate-configs --migrate-stdout

# Select a formatter when several configurations coexist
ryl --migrate-configs --migrate-from prettier --migrate-write

# Import one explicitly named configuration
ryl --migrate-configs --migrate-from yamlfix --migrate-root config/yamlfix.toml --migrate-write
```

The explicit-file example writes `config/.ryl.toml`, discovered only for files under
`config/`. Move the generated file to the project root or pass `-c config/.ryl.toml`.

## After migrating

1. Review formatting with `ryl format --diff .`, then apply with `ryl format .`.
2. Formatter-only imports enable no lint rules: `ryl check .` exits `2`. Enable
   selected rules in TOML or run `ryl check --enable key-duplicates .` to start linting.
3. [Convert suppression comments](#suppression-comments), then remove obsolete
   configs according to [Source cleanup](#source-cleanup). Keep shared configs for
   other languages.
4. Replace the old YAML formatter command in CI with `ryl format --check .`.

`--migrate-from` accepts `yamllint`, `prettier`, `yamlfmt`, `dprint`, and `yamlfix`.
The `pretty-yaml` alias selects dprint. Selection affects project migration;
user-global migration remains a yamllint conversion.

The importer was checked against Prettier 3.9.10,
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

| Situation | Migration behavior |
| :--- | :--- |
| yamllint config in the same directory | Combine lint and formatter settings; explicit formatter-file cleanup leaves the lint source alone |
| Existing ryl config | Refuse to overwrite; export with `--migrate-stdout` and merge by hand (exit `2`) |
| Child formatter config would hide inherited yamllint rules | Skip and warn; combine settings manually (exit `2`) |
| Independent directories or nested formatter-only configs | Migrate each eligible config |

Formatter width and indentation use top-level settings; lint thresholds remain independent.
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

Convert configs first, then comments; comment conversion also works with an existing
ryl config:

1. Preview comment changes with the first command below.
2. Apply with `--migrate-write` after reviewing the diff and warnings.
3. Run `ryl format --diff .` and, with lint rules enabled, `ryl check .`.

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
| `# prettier-ignore` | `# ryl format-ignore` when the comment protects the same YAML node |
| pretty_yaml's configured ignore marker | `# ryl format-ignore` when the comment protects the same YAML node |
| First-line `# !yamlfmt!:ignore` | `# ryl format-disable` for one document without an explicit end marker |

Only actual YAML comments change, including supported Markdown YAML regions. Scalar
contents and unrelated bytes remain unchanged. Formatter conversions preserve formatting
only; lint and safe lint fixes remain active. No additional runtime aliases are installed.

Retained comments are left in their original spelling with a warning. Review them
manually when migration cannot prove which YAML node or region a comment protects:

- Inline markers, reason suffixes, tags or anchors, explicit keys, flow members,
  document crossings, or existing ryl formatter directives.
- Markers shared with lint directives, or source per-file, plugin or pragma settings
  that change the protected region. A non-first-line `# !yamlfmt!:ignore` has no
  proven equivalent; choose a native node or region directive manually.
- Prettier Markdown outside supported front matter and lowercase `yaml` fences.
  Embedded formatting and a matching parser must be enabled; Quarto and R Markdown
  need an explicit data-only Markdown parser configuration.
- Dprint plugins other than official pretty_yaml `0.5.x` or `0.6.0` (official WASM
  URLs may include checksums), remote inheritance, or custom file associations.
- Shared comment segments outside the supported legacy directive grammar.

YAML-only Prettier overrides without parser, plugin, or pragma options permit YAML
comment conversion. Overrides provably limited to other literal extensions, such
as `*.md`, do not block YAML conversion. Other overrides retain source markers.
Every directive run reports converted and retained comment totals plus skipped
file counts; reruns still report unsupported retained markers.

Writes preserve supported encodings and BOMs. Files whose bytes cannot round-trip
through their encoding are retained. Previews report when encodings, line endings or
filenames prevent a text diff. Source configs are excluded from application writes;
symlinks stay unchanged. Combined migration cleans up eligible configs only after
application writes succeed.

## Source cleanup

| Source | `--migrate-delete-old` / `--migrate-rename-old` |
| :--- | :--- |
| yamllint, recognized standalone yamlfmt or yamlfix | Eligible after successful import with understood settings |
| Prettier, dprint, `package.json`, `package.yaml`, `pyproject.toml` | Always retained because they can configure other tools; remove obsolete YAML settings manually |

Custom filenames, sources with unknown settings, skipped sources, and inherited
sources outside the migration root remain untouched, with a warning.
