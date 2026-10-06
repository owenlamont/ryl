---
name: config-discovery
description: >-
  Use when changing where ryl looks for configuration, the precedence between
  sources, or the explicit-opt-in rule that makes an unconfigured run exit 2.
  Covers `-d`/`-c`/project/`YAMLLINT_CONFIG_FILE`/user-global ordering, the
  TOML-first candidate walk, and the two stricter-than-yamllint exits.
---

# Config Discovery

## Precedence

`config::discover_config_with`, high→low: `-d` (inline YAML) > `-c` (file: TOML/YAML by
extension) > project config > `YAMLLINT_CONFIG_FILE` > user-global. Precedence diagram in
`docs/getting-started/quickstart.md`.

`-d`/`-c`/a present `YAMLLINT_CONFIG_FILE` trigger run-wide resolution
(`main::build_global_cfg`); otherwise project + user-global discovery is per file
(`cli_support::resolve_ctx`), so a monorepo gets a config per subtree. `ConfigCache`
caches by directory and project configs by file path, so directories sharing one project
config load it once and share one `Arc`; a user-global fallback is cached per directory
only. Run-wide
resolution still applies the full precedence, so a project config found from the inputs
precedes the env config.

## Candidate walk

Project candidates run every ancestor to `HOME`, TOML-first
(`TOML_PROJECT_CONFIG_CANDIDATES`: `.ryl.toml` > `ryl.toml` > `.config/.ryl.toml` >
`.config/ryl.toml` > `pyproject.toml [tool.ryl]`) across every ancestor first, then
`.yamllint*` in a separate full ancestor walk (`find_first_yaml_candidate`), so any TOML
config up-tree outranks even a nearer `.yamllint`.

`.config/` is TOML-only and anchors path globs/`ignore-from-file` at its parent
(`config_base_dir`, #218). `YAMLLINT_CONFIG_FILE` is yamllint-only: a `.toml` target
errors (exit 2) before the existence check (`try_env_config_core`, #332); use
`-c`/`-d`/project discovery for ryl TOML. User-global: ryl-native
`<config-dir>/ryl/.ryl.toml` or `ryl.toml`, then yamllint `<config-dir>/yamllint/config`.

## Notices and deprecated TOML keys

`ConfigContext::notices` are stderr warnings, silenced by `--no-warnings` and shown in the
LSP via `window/showMessage`. `finalize_context` adds one per deprecated TOML key
(`config_schema::DEPRECATED_TOML_KEYS`, read by `TomlConfig::deprecated_keys`, which
`--migrate-configs`/`--migrate-user-config` also use to pick the TOML files they rewrite
in place; the warning names whichever of the two applies to its file).
`TomlConfig::merged_lint` resolves each key, the `[lint]` spelling winning; a new
deprecated key needs a row in the table, a pair in `deprecated_keys`, and a line in
`merged_lint`. The legacy YAML config stays flat.

The top-level `line-length`/`indent-width` are TOML-only and never deprecated: an
explicit `[lint.rules.<rule>]` option beats them, and they beat the rule's built-in
default (`indentation.spaces` stays `consistent` when `indent-width` is unset).

## Nothing is enabled implicitly

ryl never enables a rule that wasn't explicitly turned on (no "default-on" rules). Two
cases exit `2`, both stricter than yamllint:

- **No config found anywhere** — resolution falls back to an *empty* config
  (`ConfigContext::config_found == false`), not the `default` preset; reports
  `main::NO_CONFIG_ERROR`. yamllint lints with `extends: default`.
- **A resolved config that enables no rules** — `rules: {}`, empty
  `[lint.rules]`/`[tool.ryl]`, a `[files]`-only TOML config, or one disabling everything;
  reports `main::NO_RULES_ENABLED_ERROR`. yamllint silently lints nothing.

Both via `YamlLintConfig::enables_any_rule`; `main::no_rules_error(config_found)` picks
the message. The `default`/`relaxed`/`empty` presets stay available via `extends:` (YAML
only). `--migrate-configs` (warns instead) and `--list-files` are exempt.

`--enable <RULES>` counts as explicitly turning rules on: `CliConfigFlags::apply` runs
`YamlLintConfig::restrict_rules` on every resolved config (global, per-file before
caching, stdin), including the empty no-config fallback, so neither exit fires.
