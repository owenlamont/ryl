---
name: ryl
description: >-
  Fast YAML linter and formatter. Use when checking or formatting YAML, setting
  up YAML tooling, or migrating from yamllint. Covers rule selection, exit codes,
  safe fixes versus formatting, TOML configuration, output formats, directives,
  and YAML embedded in Markdown.
license: MIT
---

# ryl

ryl is a fast YAML linter and formatter built for programmatic use: stable exit
codes, machine-readable output, in-place edits, and stdin support.

Install: `pip install ryl`, `cargo install ryl`, or `npm i -g @owenlamont/ryl`.
Pre-commit via `owenlamont/ryl-pre-commit`: `ryl-check` and `ryl-format`, plus
`ryl-check-markdown` and `ryl-format-markdown` for embedded YAML.

## Critical: ryl has no default-on rules

ryl never enables a rule unless a configuration or `--enable` turns it on. When it has a
file to lint and neither `--enable` nor a config that enables a rule, it exits `2`, not
`0` (an empty or fully-ignored input set still exits `0`). Always give it a
config or `--enable` first:

```bash
ryl check --enable ALL .                    # quick: every rule, including ryl-only rules
ryl check --enable truthy,line-length .     # run only these rules, config or not
```

`--enable` takes comma-separated rule IDs or `ALL` (every rule, ryl-only ones included)
and replaces the config's rule selection for that run.

Or commit a `ryl.toml` / `.ryl.toml` (in `pyproject.toml`, prefix the tables with
`tool.ryl`, so `[lint.rules]` becomes `[tool.ryl.lint.rules]`):

```toml
# ryl.toml
[lint.rules]
trailing-spaces = "enable"
new-line-at-end-of-file = "enable"
```

`ALL = "enable"` under `[lint.rules]` turns on every rule at its defaults; an explicit entry
for a rule wins over it.

## Run and branch on exit codes

```bash
ryl check <path|file ...>    # files and/or directories; `ryl check .` recurses, honouring .gitignore
ryl check --list-files .      # preview which files would be linted, then exit
```

Exit codes from a plain lint run (`--fix`/`--diff` differ, see below):

- `0`: no errors (warning-level findings still exit `0` unless `--strict`).
- `1`: error-level findings, invalid YAML, or an unreadable path.
- `2`: usage error (no inputs, bad flags), no rules enabled by config or `--enable`, or
  `--strict` with only warnings.

Add `--strict` to make warnings fail (exit `2`); `--no-warnings` reports only errors.

## Fix or preview

- `ryl check --fix` applies safe lint fixes in place, then reports remaining problems.
- `ryl check --diff` previews those fixes as a unified diff on stdout. It writes
  nothing and exits `1` if any file would change.
- `--diff` only previews: files it can't represent as a text diff are skipped with a
  notice and don't change the exit code. `--fix` writes in place, but an unparsable file
  is reported (not rewritten) and still exits `1`.

`ryl format` formats YAML in place without enabling lint rules. Use `ryl format --diff`
to preview a patch without writing; it exits `1` if formatting would change a file.
The formatter also has a check mode that reports pending changes without a patch.
It reads `[format]` in TOML and handles Markdown with `--markdown`.

## Machine-readable output

`ryl check -f/--output-format`: `parsable` or `github` (line-oriented, stderr) for parsing
diagnostics; `junit` or `gitlab` (stdout) for CI report artifacts. `--output-format` is
repeatable and each
`-o/--output-file` binds to the preceding format, so console + a report file can be
produced together:

```bash
ryl check --output-format github --output-format gitlab -o code-quality.json .
```

Formatter check mode accepts one `--output-format`; it has no `-f`, `--format`, or
`-o` alias. See the [formatter guide](https://ryl-docs.pages.dev/formatter/) for
the check-mode command.

## Configuration: YAML vs TOML

- **TOML** (`ryl.toml`, `.ryl.toml`, `[tool.ryl]`) is the native format and holds
  `[lint.rules]`, `[format]`, `[files]` globs, `[markdown]` embedding, `[output]`
  destinations, per-line ignores, and ryl-only rules.
- **YAML** (`.yamllint`, `YAMLLINT_CONFIG_FILE`) is deprecated but remains
  yamllint-compatible (`extends: default`/`relaxed`, presets). Loading YAML prints
  a migration warning. `YAMLLINT_CONFIG_FILE` takes a YAML config only: a
  `.toml` target errors (use `-c`/`-d` or project discovery for ryl TOML).

Migrate a yamllint setup with `ryl --migrate-configs` (add `--migrate-write` to apply).

## Stdin, directives, Markdown

- `-` reads stdin; pair with `--stdin-filename <PATH>` so diagnostics, config discovery,
  and filtering behave as if the path were on disk.
- Suppress rules inline with `# ryl disable`/`enable`/`disable-line`, or a first-line
  `# ryl disable-file`; a `# ryl` directive may share a line with other `#` comments.
- `--markdown` lints YAML front matter and fenced `yaml`/`yml` blocks in Markdown.

## Full documentation

- Agent guide: <https://ryl-docs.pages.dev/using-ryl-with-ai-agents/>
- Quick start: <https://ryl-docs.pages.dev/getting-started/quickstart/>
- Migrating from yamllint: <https://ryl-docs.pages.dev/getting-started/migrating-from-yamllint/>
- Output formats: <https://ryl-docs.pages.dev/output-formats/>
- Rules: <https://ryl-docs.pages.dev/rules/>
