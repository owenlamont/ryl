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
| dprint | `dprint.json`, `dprint.jsonc`, `.dprint.json`, `.dprint.jsonc`, in that order; a pretty_yaml plugin or `yaml` section identifies YAML settings |
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
configuration blocks migration. A formatter-only child configuration is also skipped
when a new target would hide inherited yamllint rules; migration retains the source
and warns you to combine lint and formatter settings manually. Independent directories
and nested formatter-only configurations can still migrate.
Preview, atomic writes, source symlink refusal,
and backup collision checks follow the
[yamllint migration safeguards](migrating-from-yamllint.md#automatic-migration).

## Effective settings and gaps

Migration writes representable source defaults as well as explicit options.
yamlfmt imports follow its numeric coercions and effective encoder defaults,
including legacy octal numbers and fractional integer truncation.
Generated `[format]` settings include quote presentation, prose wrapping,
line endings, collection policies, comments, blank lines, and document markers
where supported. Formatter-only imports enable no lint rules.

Unsupported settings produce warnings on stderr and comments in generated TOML.
Migration preserves YAML values and properties rather than adopting destructive
source behavior. Migration also keeps ryl's default
[YAML 1.1 reader compatibility](../yaml-version.md); a source formatter does not
establish which YAML readers consume your files. Source ignore comments are left unchanged; config migration
does not add aliases for another formatter's directives.

| Gap | Migration behavior |
| :--- | :--- |
| Forced quoting of plain strings | Retains existing quote presentation; warns when forcing was requested |
| Per-file overrides or shared configurations | Imports static root settings only; warns |
| Source-specific flow wrapping, bracket padding, or trailing commas | Uses supported ryl layout settings; warns about differences |
| yamlfmt YAML merge keys | Retains the source without importing formatter settings; expand merged settings manually and rerun |
| yamlfmt scalar spellings requiring unsupported resolution | Retains the source without importing formatter settings; quote string values, use plain decimal numbers, or configure ryl manually |
| Removing document-start markers | Preserves existing markers; warns |
| yamlfix null spelling and per-style prose behavior | Preserves null spellings and uses split-only line folding; warns |
| Dimensions outside ryl's supported range | Uses a supported fallback and warns |

## Source cleanup

`--migrate-delete-old` and `--migrate-rename-old` retain all Prettier and dprint
configurations, `package.json`, `package.yaml`, and `pyproject.toml`, because
those sources can configure other languages or tools. Only recognized standalone
yamlfmt and yamlfix filenames with understood settings are eligible for cleanup.
Custom filenames, sources with unknown settings, skipped sources, and inherited
sources outside the migration root remain untouched, with a warning.
