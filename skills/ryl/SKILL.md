---
name: ryl
description: >-
  Lint and auto-fix YAML with the ryl CLI (a fast, yamllint-compatible linter).
  Use when running or configuring ryl in a repo, fixing YAML lint errors, setting
  up YAML linting, or migrating from yamllint. Covers the no-default-on-rules
  gotcha (ryl exits 2 without a config), exit codes, --fix vs --diff,
  machine-readable output, the YAML-vs-TOML config split, inline directives, and
  YAML embedded in Markdown.
license: MIT
---

# ryl

ryl is a fast YAML linter (Rust, yamllint-compatible) built for programmatic use:
stable exit codes, machine-readable output, in-place fixes, and stdin support.

Install: `pip install ryl`, `cargo install ryl`, or `npm i -g @owenlamont/ryl`.
Pre-commit via the `owenlamont/ryl-pre-commit` hook (`ryl` for YAML files,
`ryl-markdown` for embedded YAML).

## Critical: ryl has no default-on rules

ryl never enables a rule unless a configuration turns it on. When it has a file to lint
but no config (or a config that enables zero rules) it exits `2`, not `0` (an empty or
fully-ignored input set still exits `0`). This is stricter than yamllint. Always give it
a config first:

```bash
ryl -d 'extends: default' .          # quick: yamllint's standard rule set
```

Or commit a `ryl.toml` / `.ryl.toml` (in `pyproject.toml`, prefix the tables with
`tool.ryl`, so `[rules]` becomes `[tool.ryl.rules]`):

```toml
# ryl.toml
[rules]
trailing-spaces = "enable"
new-line-at-end-of-file = "enable"
```

## Run and branch on exit codes

```bash
ryl <path|file ...>     # files and/or directories; `ryl .` recurses, honouring .gitignore
ryl --list-files .      # preview which files would be linted, then exit
```

Exit codes from a plain lint run (`--fix`/`--diff` differ, see below):

- `0`: no errors (warning-level findings still exit `0` unless `--strict`).
- `1`: error-level findings, invalid YAML, or an unreadable path.
- `2`: usage error (no inputs, bad flags), no config / no rules enabled, or `--strict`
  with only warnings.

Add `--strict` to make warnings fail (exit `2`); `--no-warnings` reports only errors.

## Fix or preview

- `--fix` applies safe fixes in place, then reports remaining problems.
- `--diff` previews the fixes (unified diff on stdout), writes nothing, exits `1` if any
  file would change. Use `--diff` for a non-mutating check, `--fix` to apply.
- `--diff` only previews: files it can't represent as a text diff are skipped with a
  notice and don't change the exit code. `--fix` writes in place, but an unparsable file
  is reported (not rewritten) and still exits `1`.

## Machine-readable output

`-f/--format`: `parsable` or `github` (line-oriented, stderr) for parsing diagnostics;
`junit` or `gitlab` (stdout) for CI report artifacts. `--format` is repeatable and each
`-o/--output-file` binds to the preceding format, so console + a report file can be
produced together:

```bash
ryl --format github --format gitlab -o code-quality.json .
```

## Configuration: YAML vs TOML

- **YAML** (`.yamllint`, `YAMLLINT_CONFIG_FILE`) is yamllint-compatible (`extends:
  default`/`relaxed`, presets).
- **TOML** (`ryl.toml`, `.ryl.toml`, `[tool.ryl]`) holds ryl-only features: `[files]`
  globs, `[markdown]` embedding, `[output]` destinations, per-line ignores, ryl-only
  rules.

Migrate a yamllint setup with `ryl --migrate-configs` (add `--migrate-write` to apply).

## Stdin, directives, Markdown

- `-` reads stdin; pair with `--stdin-filename <PATH>` so diagnostics, config discovery,
  and filtering behave as if the path were on disk.
- Suppress rules inline with `# ryl disable`/`enable`/`disable-line`, or a first-line
  `# ryl disable-file`.
- `--markdown` lints YAML front matter and fenced `yaml`/`yml` blocks in Markdown.

## Full documentation

- Agent guide: <https://ryl-docs.pages.dev/using-ryl-with-ai-agents/>
- Quick start: <https://ryl-docs.pages.dev/getting-started/quickstart/>
- Migrating from yamllint: <https://ryl-docs.pages.dev/getting-started/migrating-from-yamllint/>
- Output formats: <https://ryl-docs.pages.dev/output-formats/>
- Rules: <https://ryl-docs.pages.dev/rules/>
