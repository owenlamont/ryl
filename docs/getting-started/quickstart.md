# Quick start

## Run a lint

Point ryl at a file or directory:

```bash
# Lint a single file
ryl path/to/file.yaml

# Lint a project (recursively scans .yml/.yaml, honouring .gitignore)
ryl .
```

## Lint from stdin

Pass `-` as the input to read YAML from standard input &mdash; useful for
editor integrations where the buffer is not yet on disk:

```bash
cat file.yaml | ryl -

# Provide a filename so diagnostics, config discovery, and
# yaml-files / per-file-ignores match the right path:
cat file.yaml | ryl - --stdin-filename path/to/file.yaml
```

Without `--stdin-filename`, diagnostics are labelled `<stdin>`, config
discovery is anchored at the current working directory, and all
path-based filtering (`yaml-files`, per-file-ignores, per-rule `ignore`
patterns) is skipped so every enabled rule runs. `-` cannot be combined
with other inputs or with `--fix`.

Exit codes:

- `0` &mdash; no problems found.
- `1` &mdash; lint errors, invalid YAML, or a path that could not be read
  (including nonexistent files).
- `2` &mdash; CLI usage error (no inputs provided, bad flags), or
  `--strict` was set and only warnings were produced.

A configuration that enables no rules would lint nothing, so ryl rejects it with
exit `2` rather than silently passing. Enable at least one rule, or remove the
configuration to fall back to the default rule set. (This is stricter than yamllint,
which silently accepts a rule-less config.)

## Apply auto-fixes

ryl can automatically fix a subset of rules:

```bash
ryl --fix .
```

See the [Rules reference](../rules.md) for which rules are fixable.

`--fix` rewrites files in place but never writes through a symlink: a
symlinked input is linted but skipped for fixing (with a warning on
stderr), so a symlink in an untrusted tree cannot redirect a write to a
file outside it. This mirrors directory scanning, which does not follow
symlinks.

## Configure for your project

Drop a `.ryl.toml` (or `ryl.toml`) at the root of your repo. TOML
configuration is flat &mdash; copy the preset you want from
[Configuration presets](../config-presets.md) and customise from there:

```toml
yaml-files = ["*.yaml", "*.yml", ".yamllint"]

# ... rule enable/disable table from the preset ...

[rules.line-length]
max = 120
allow-non-breakable-words = true
```

YAML configuration is also accepted for parity with yamllint and supports
`extends:` for selecting a preset. Both `.yamllint` and `.ryl.toml` are
discovered automatically. TOML is the recommended format for ryl-specific
features (such as fix selection) that have no upstream yamllint
equivalent.

If you already have a yamllint configuration, use the built-in converter:

```bash
ryl --migrate-configs --migrate-write
```

See [Migrating from yamllint](migrating-from-yamllint.md) for details.
