# Quick start

## Run a lint

Point ryl at a file or directory:

```bash
# Lint a single file
ryl path/to/file.yaml

# Lint a project (recursively scans .yml/.yaml, honouring .gitignore)
ryl .
```

ryl does not enable any rules by default, so these commands report `no
configuration found` (exit `2`) until a configuration enables at least one rule.
To lint with yamllint's standard rule set straight away, pass it inline:

```bash
ryl -d 'extends: default' .
```

or drop a config in your project (see [Configure for your
project](#configure-for-your-project) below).

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
with other inputs, and `--fix` cannot read from stdin (use `--diff` to
preview fixes instead).

Exit codes:

- `0` &mdash; no problems found.
- `1` &mdash; lint errors, invalid YAML, or a path that could not be read
  (including nonexistent files).
- `2` &mdash; CLI usage error (no inputs provided, bad flags), or
  `--strict` was set and only warnings were produced.

ryl never enables a rule unless a configuration explicitly turns it on, so two
cases exit `2` rather than silently linting nothing:

- **No configuration found** anywhere (no `-c`/`-d`, no `YAMLLINT_CONFIG_FILE`, no
  discovered `.ryl.toml`/`.yamllint`). Create a config that enables rules, or pass a
  YAML config with `extends: default` for yamllint's standard rule set.
- **A configuration that enables no rules** (`rules: {}`, an empty
  `[rules]`/`[tool.ryl]`, or one disabling everything). Enable at least one rule, or
  use `extends: default`.

This is stricter than yamllint, which lints with the `default` preset when no config
is found and silently accepts a rule-less config. Give ryl a config containing
`extends: default` to reproduce yamllint's out-of-the-box behaviour.

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

## Preview fixes as a diff

`--diff` runs the same safe fixes as `--fix` but, instead of writing,
prints a unified diff (3 lines of context) of what would change to
stdout &mdash; modelled on `ruff check --diff`:

```bash
ryl --diff .
```

This is handy for CI previews, PR review, and parallel-safe runners such
as [hk](https://hk.jdx.dev) that apply the diff themselves rather than
re-invoking the linter. `--diff` never modifies files, is mutually
exclusive with `--fix`, and (unlike `--fix`) works with `-`/stdin.

Like `ruff check --diff`, the exit code reflects only the diff &mdash;
remaining *unfixable* findings are neither printed nor counted:

- `1` &mdash; at least one file would change.
- `0` &mdash; no file would change.
- `2` &mdash; CLI usage error.

A file that cannot be parsed (or a symlink) is skipped with a notice on
stderr and does not affect the exit code. A non-UTF-8 or BOM-prefixed file
is also skipped: a textual diff of its decoded content could not be applied
back to the original bytes, so use `--fix` (which preserves the encoding)
for those. For embedded YAML in Markdown, the diff is reported at the
host-file level (one diff per `.md`).

## Configure for your project

Drop a `.ryl.toml` (or `ryl.toml`) at the root of your repo. TOML
configuration is flat &mdash; copy the preset you want from
[Configuration presets](../config-presets.md) and customise from there:

```toml
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

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
