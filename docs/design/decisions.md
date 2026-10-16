# Design decisions

These are current, load-bearing decisions confirmed by the maintainer. Follow the linked discussion before changing one. [Scope](../scope.md) records proposals ryl declines; the [migration guide](../getting-started/migrating-from-yamllint.md#how-ryl-differs-from-yamllint) records compatibility differences.

## Configuration

- **Rules require explicit enablement.** An unconfigured run exits with an error instead of silently applying the yamllint default preset. This keeps the active rule set visible in local configuration. [Decision](https://github.com/owenlamont/ryl/issues/248)
- **TOML is the native configuration model.** Shared settings live at the top level, lint settings under `[lint]`, and formatting targets under `[format]`. The nested shape replaced the original flat lint configuration; migration remains available. [Formatter epic](https://github.com/owenlamont/ryl/issues/374), [migration](https://github.com/owenlamont/ryl/pull/508)
- **Legacy yamllint YAML configuration is on a deprecation path.** Migration remains available, and a removal release has not been set. The native TOML model supersedes treating yamllint YAML as a permanent peer format. [Decision](https://github.com/owenlamont/ryl/issues/478), [warning](https://github.com/owenlamont/ryl/pull/562)
- **A warning names a usable migration target.** Deprecation warnings start only after the replacement exists; old configuration must not silently stop applying. [Formatter epic](https://github.com/owenlamont/ryl/issues/374)
- **Migrated user configuration remains self-contained.** Relative includes are resolved against their original config before migration, preserving behaviour from another working directory. [Review decision](https://github.com/owenlamont/ryl/pull/307)

## Rules

- **Rules cover general YAML and evidence-backed practices.** Application-specific schemas and contested style choices stay outside the built-in rule set. [Scope](../scope.md), [rules epic](https://github.com/owenlamont/ryl/issues/261)
- **Safe fixes preserve resolved values, comments, and anchors.** An ambiguous repair stays a diagnostic; a meaning-changing repair would require explicit opt-in. [Scope](../scope.md), [fix investigation](https://github.com/owenlamont/ryl/issues/147)
- **Layout rules remain available in `ryl check`.** The formatter also enforces layout, and warns when enabled lint options conflict with its targets. This superseded the original plan to move layout diagnostics solely into formatting. [Maintainer re-review](https://github.com/owenlamont/ryl/issues/374)

## Formatter

- **Formatting uses local source edits, not whole-document reprinting.** The result must preserve resolved values and comment and anchor attachment, remain idempotent, and leave unsafe cases unchanged with a report. [Formatter epic](https://github.com/owenlamont/ryl/issues/374), [scope](../scope.md)
- **`[format]` has one target per option; lint options remain tolerances.** The formatter has its own pass list and does not inherit lint rule selection. This replaced the earlier proposal to move each layout rule's options into `[format]`. [Maintainer re-review](https://github.com/owenlamont/ryl/issues/374)
- **The formatter works with built-in defaults.** Linting still requires explicit rules; formatting has an overridable best-practice YAML 1.2 profile without config inheritance. [Formatter epic](https://github.com/owenlamont/ryl/issues/374), [profile](https://github.com/owenlamont/ryl/issues/385)
- **The formatter epic landed as one integration branch after corpus validation.** The release gate checked real YAML for value changes, panics, and comment loss. This replaced an incremental release plan. [Maintainer re-review](https://github.com/owenlamont/ryl/issues/374), [validation](https://github.com/owenlamont/ryl/issues/386)

## CLI and exit codes

- **`ryl check` is the named lint command; bare paths are deprecated.** `ryl format` writes in place by default, while `--check` and `--diff` inspect without writing. The command split follows the lint and format boundary. [Formatter epic](https://github.com/owenlamont/ryl/issues/374)
- **No configuration or no enabled rules is a usage error.** Exit `2` makes accidental unconfigured runs visible; findings and invalid YAML use exit `1`. [Explicit enablement](https://github.com/owenlamont/ryl/issues/248), [CLI contract](https://github.com/owenlamont/ryl/blob/main/AGENTS.md#cli-behavior)

## Parser authority

- **YAML 1.2.2 grammar and the reference parser settle parse disagreements.** PyYAML and ruamel cannot outvote the reference event stream; resolving loaders answer core-schema type questions. Divergences from yamllint are documented. [Decision](https://github.com/owenlamont/ryl/pull/701), [migration catalog](../getting-started/migrating-from-yamllint.md#how-ryl-differs-from-yamllint)
- **Scalar identity follows the YAML 1.2 core schema.** A plain `011` is decimal eleven, superseding the proposed JSON-schema string reading. Canonical duplicate-key checks now compare integers of any width by value, superseding the earlier `i64` limit. [Maintainer review decision](https://github.com/owenlamont/ryl/pull/267), [wide-integer correction](https://github.com/owenlamont/ryl/pull/517)

## Packaging and release

- **Release assets carry attestations and SHA-256 checksums.** Users can verify the published binaries independently of a package manager. [Attestations](https://github.com/owenlamont/ryl/issues/101), [checksums](https://github.com/owenlamont/ryl/issues/322)
- **Release notes derive from a sourced changelog.** User-facing changes are recorded with issue or PR links before publication. [Decision](https://github.com/owenlamont/ryl/issues/607), [changelog](https://github.com/owenlamont/ryl/blob/main/CHANGELOG.md)

## LSP and editor

- **The language server uses client-supported diagnostic delivery.** Pushing and serving pull diagnostics without regard to client capabilities caused duplicates. [Decision](https://github.com/owenlamont/ryl/issues/323)
- **Editor Format Document uses `ryl format`.** Editor formatting shares the CLI's behaviour and safety guarantees. [Decision](https://github.com/owenlamont/ryl/issues/580), [implementation](https://github.com/owenlamont/ryl/pull/583)
