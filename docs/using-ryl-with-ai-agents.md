# Using ryl with AI agents

This page is for AI coding agents (and the people configuring them) that run ryl in a
downstream repository. ryl is built for programmatic use: stable exit codes,
machine-readable output, in-place fixes, and stdin support. The
[`ryl` Agent Skill](https://github.com/owenlamont/ryl/blob/main/skills/ryl/SKILL.md)
is the tightened, trigger-oriented version of this guidance; this page is the canonical
source.

## ryl has no default-on rules (get this right first)

ryl never enables a rule unless a configuration turns it on. When it has at least one
file to lint but finds no configuration (or a configuration that enables zero rules), it
exits `2` (a usage error), not `0`. This is stricter than yamllint, which lints with its
`default` preset out of the box. (An empty or fully-ignored input set still exits `0`, so
a clean exit on a fresh repo does not by itself prove that any YAML was linted.)

So an agent dropping ryl into a project must provide a configuration first. The quickest
way to reproduce yamllint's standard rule set:

```bash
ryl -d 'extends: default' .
```

For a persistent project config, create a `ryl.toml` (or `.ryl.toml`, or a `[tool.ryl]`
table in `pyproject.toml`) that enables at least one rule:

```toml
# ryl.toml
[rules]
trailing-spaces = "enable"
new-line-at-end-of-file = "enable"
```

In `pyproject.toml`, nest these under `[tool.ryl]` (e.g. `[tool.ryl.rules]`). See
[Quick start](getting-started/quickstart.md) and [Presets](config-presets.md).

## Invocation and exit codes

```bash
ryl <path|file ...>     # files and/or directories
ryl .                   # recurse, honouring .gitignore; inputs are de-duplicated
```

Branch on the exit code:

- `0`: no problems found.
- `1`: lint errors, invalid YAML, or a path that could not be read.
- `2`: usage error (no inputs, bad flags, no configuration found, or a configuration
  that enables no rules).

These codes describe a normal lint run. `--diff` is preview-only: a file it cannot
process (unparsable, a symlink, or non-UTF-8) is skipped with a per-file notice and does
not affect the exit code. `--fix` likewise never writes such a file, but it still lints
it, so an unparsable file under `--fix` is reported and exits `1`.

Warnings alone do not fail the run (they exit `0`). To make warnings fail, pass
`--strict` (warnings then exit `2`) or raise the rule's level to `error` in the
configuration; `--no-warnings` does the opposite, reporting only errors.

## Previewing the file set

`--list-files` prints the files ryl would lint (one per line) and exits without linting,
so an agent can confirm scope before a real run:

```bash
ryl --list-files .
```

## Machine-readable output

`-f/--format` selects the format; console formats default to **stderr**, the report
formats `junit`/`gitlab` default to **stdout**:

- `parsable` (`file:line:col: [level] message`) or `github` for line-oriented parsing.
- `junit` (JUnit XML) or `gitlab` (GitLab Code Quality JSON) for CI report artifacts.

`--format` is repeatable, and each `-o/--output-file` binds to the preceding `--format`,
so a console stream and a report file can be produced in one run:

```bash
ryl --format github --format gitlab -o code-quality.json .
```

See [Output formats](output-formats.md).

## Apply or preview fixes

- `--fix` applies safe fixes in place, then reports any remaining (unfixable) problems.
- `--diff` previews those fixes as a unified diff on stdout without writing, and exits
  `1` if any file would change.

Use `--diff` for a non-mutating CI check and `--fix` to apply. A file that does not fully
parse, or is reached through a symlink, is skipped (reported, never written).

## Reading from stdin

`-` reads one document from stdin. Pair it with `--stdin-filename <PATH>` so diagnostics,
config discovery, source-kind resolution, and per-file filtering all behave as if that
path were on disk:

```bash
cat config.yaml | ryl --stdin-filename config.yaml -
```

## Configuration: YAML vs TOML

ryl reads two configuration dialects:

- **YAML** (`.yamllint`, `.yamllint.yaml`, `YAMLLINT_CONFIG_FILE`) is yamllint-compatible,
  including `extends: default`/`relaxed` and rule presets.
- **TOML** (`ryl.toml`, `.ryl.toml`, `[tool.ryl]` in `pyproject.toml`) is the home for
  ryl-only features: `[files]` source globs, `[markdown]` embedding, `[output]`
  destinations, per-line ignores, and ryl-only rules.

Migrate an existing yamllint setup with `ryl --migrate-configs` (preview;
`--migrate-write` to apply), and a user-global yamllint config with
`ryl --migrate-user-config`. See
[Migrating from yamllint](getting-started/migrating-from-yamllint.md) and
[Presets](config-presets.md).

## Inline directives

Suppress rules from within a file with comment directives (`# ryl disable`, `enable`,
`disable-line`, and a first-line `# ryl disable-file`). See
[Inline directives](directives.md).

## YAML embedded in Markdown

`--markdown` lints YAML front matter and fenced `yaml`/`yml` blocks inside Markdown files
(also configurable per-project via `[files].markdown`). See
[YAML in Markdown](markdown.md).

## Further reading

- [Quick start](getting-started/quickstart.md)
- [Migrating from yamllint](getting-started/migrating-from-yamllint.md)
- [Rules](rules.md)
- [Presets](config-presets.md)
- [Output formats](output-formats.md)
- [Inline directives](directives.md)
- [Per-line ignores](per-line-ignores.md)
- [YAML in Markdown](markdown.md)
