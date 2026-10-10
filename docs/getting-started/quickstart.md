# Quick start

## The `ryl check` subcommand

ryl's CLI is moving to subcommands: `ryl check` is the lint pass and `ryl format` is
the [formatter](../formatter.md). `ryl check <paths>` is the recommended form and
is used throughout these docs. Bare `ryl <paths>` still lints identically, but it is
deprecated — it prints a warning to stderr (silenced by `--no-warnings`) and a later
release will remove it, so adopt `ryl check` now.

## Run a lint

Point ryl at a file or directory:

```bash
# Lint a single file
ryl check path/to/file.yaml

# Lint a project (recursively scans .yml/.yaml, honouring .gitignore)
ryl check .
```

ryl does not enable any rules by default, so these commands report `no
configuration found` (exit `2`) until a configuration or `--enable` turns on a rule.
To lint with yamllint's standard rule set straight away, pass it inline:

```bash
ryl check -d 'extends: default' .
```

or name the rules to run with `--enable`, which needs no config file:

```bash
ryl check --enable ALL .
```

Or drop a config in your project (see [Configure for your
project](#configure-for-your-project) below).

## Lint from stdin

Pass `-` as the input to read YAML from standard input &mdash; useful for
editor integrations where the buffer is not yet on disk:

```bash
cat file.yaml | ryl check -

# Provide a filename so diagnostics, config discovery, and
# yaml-files / per-file-ignores match the right path:
cat file.yaml | ryl check - --stdin-filename path/to/file.yaml
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

ryl never enables a rule unless a configuration or `--enable` explicitly turns it on, so two
cases exit `2` rather than silently linting nothing:

- **No configuration found** anywhere (no `-c`/`-d`, no `YAMLLINT_CONFIG_FILE`, no
  discovered `.ryl.toml`/`.yamllint`) and no `--enable`. Create a config that enables rules, pass a
  YAML config with `extends: default` for yamllint's standard rule set, or pass
  `--enable ALL`.
- **A configuration that enables no rules** (`rules: {}`, an empty
  `[lint.rules]`/`[tool.ryl]`, or one disabling everything) and no `--enable`. Enable at
  least one rule, use `extends: default`, or pass `--enable ALL`.

This is stricter than yamllint, which lints with the `default` preset when no config
is found and silently accepts a rule-less config. Give ryl a config containing
`extends: default` to reproduce yamllint's out-of-the-box behaviour.

## Apply auto-fixes

ryl can automatically fix a subset of rules:

```bash
ryl check --fix .
```

See the [Rules reference](../rules.md) for which rules are fixable.

`--fix` rewrites files in place but never writes through a symlink: a
symlinked input is linted but skipped for fixing (with a warning on
stderr), so a symlink in an untrusted tree cannot redirect a write to a
file outside it. This mirrors directory scanning, which does not follow
symlinks.

## Format

`ryl format` rewrites files to a consistent layout, and needs no config:

```bash
ryl format .
ryl format --check .
```

See [Formatter](../formatter.md) for what it changes and how to configure it.

## Preview fixes as a diff

`--diff` runs the same safe fixes as `--fix` but, instead of writing,
prints a unified diff (3 lines of context) of what would change to
stdout &mdash; modelled on `ruff check --diff`:

```bash
ryl check --diff .
```

This is handy for CI previews, PR review, and parallel-safe runners such
as [hk](https://hk.jdx.dev) that apply the diff themselves rather than
re-invoking the linter. `--diff` never modifies files, is mutually
exclusive with `--fix`, and (unlike `--fix`) works with `-`/stdin.

Diff headers use paths relative to the current directory for files beneath it.
On Windows, headers use forward slashes and omit the `\\?\` verbatim prefix,
so the patch applies with `git apply -p0`. `ryl format --diff` uses the same headers.
Filenames retain trailing dots and spaces, including on Windows.

Like `ruff check --diff`, the exit code reflects only the diff &mdash;
remaining *unfixable* findings are neither printed nor counted:

- `1` &mdash; at least one file would change.
- `0` &mdash; no file would change.
- `2` &mdash; CLI usage error.

A file that cannot be parsed (or a symlink) is skipped with a notice on
stderr and does not affect the exit code. Non-UTF-8 and BOM-prefixed files
emit no patch, but still exit `1` when a fix would change the decoded text; use `--fix`
(see [File encodings](#file-encodings)). For embedded YAML in Markdown,
the diff is reported at the host-file level (one diff per `.md`).

## File encodings

ryl auto-detects UTF-8 (with or without a BOM), UTF-16 LE/BE, and UTF-32 LE/BE:
the encodings required by [YAML 1.2](https://yaml.org/spec/1.2.2/#52-character-encodings).
A BOM identifies the encoding; without a BOM, null-byte patterns identify
UTF-16/32, otherwise ryl uses UTF-8.

Other encodings, such as Latin-1, fail with a decode error unless named via
`YAMLLINT_FILE_ENCODING` (for example, `latin-1`). Prefer converting to UTF-8;
the yamllint-compatible override prints:

> YAMLLINT_FILE_ENCODING is meant for temporary workarounds. It may be removed
> in a future version of yamllint.

`ryl check --fix` and `ryl format` preserve the original encoding and BOM;
`ryl format -` preserves stdin's encoding in stdout.

For BOM/UTF-16/UTF-32 input, `--diff` reports that no applicable text patch
can be emitted. `ryl check --diff` still exits `1` when a fix would change
the decoded text, including stdin and embedded YAML in Markdown.
`ryl format --check` and `ryl format --diff` likewise exit `1` when formatting
would change the text.

[LSP position encoding](../editor-integration.md#notes) counts columns,
independently of file encoding.

## Configure for your project

The recommended TOML config is deliberately **explicit** and **local**: it has
no default-on rules and no `extends`/inheritance, so a single `.ryl.toml`
(discovered by searching upward, and preferred over a `.yamllint`) is the
entire ruleset for the files beneath it; a monorepo can have many, each
governing its subtree. A yamllint-style YAML config instead keeps yamllint
semantics, where `extends:` merges in a preset or another file. When no
project config is found, ryl falls back to a single user-global config (see
below). Either way there are no default-on rules, so a config that enables
nothing exits `2` without `--enable` rather than silently linting nothing.

Drop a `.ryl.toml` (or `ryl.toml`) at the root of your repo. Settings shared by
every pass (`[files]`, `exclude`/`exclude-from-file`, `[markdown]`, `locale`,
`[output]`, `line-length`, `indent-width`) sit at the top level; linter settings sit under `[lint]`
(`[lint.rules]`, `fixable`/`unfixable`, `[lint.per-file-ignores]`,
`[[lint.per-line-ignores]]`); `[format]` holds the [formatter's](../formatter.md)
settings. Copy the
preset you want from [Configuration presets](../config-presets.md) and customise
from there:

```toml
[files]
yaml = [
    "*.yaml",
    "*.yml",
    ".yamllint",
]

# ... rule enable/disable table from the preset ...

[lint.rules.line-length]
max = 120
allow-non-breakable-words = true
```

The top-level `line-length` (1 to 65535) and `indent-width` (1 to 255) are the
formatter's targets and the defaults for `[lint.rules.line-length] max` and
`[lint.rules.indentation] spaces`; an explicit rule option overrides them for
linting only. Unset, `max` is 80, `spaces` is `"consistent"`, and `ryl format` keeps
each file's own indent width. A YAML config cannot set them.

```toml
line-length = 100
indent-width = 4
```

YAML configuration is also accepted for parity with yamllint and supports
`extends:` for selecting a preset. Both `.yamllint` and `.ryl.toml` are
discovered automatically. TOML is the recommended format for ryl-specific
features (such as fix selection) that have no upstream yamllint
equivalent.

To keep config out of the project root, ryl also discovers a ryl-native TOML
config inside a repo-local `.config/` directory (the RuboCop/rumdl convention).
At each directory in the upward search the candidates are tried in this order,
and the first match wins:

1. `.ryl.toml`
2. `ryl.toml`
3. `.config/.ryl.toml`
4. `.config/ryl.toml`
5. `pyproject.toml` (only when it has a `[tool.ryl]` table)

`.config/` holds ryl-native TOML only: the legacy `.yamllint`/`.yamllint.yaml`/
`.yamllint.yml` files are discovered at the directory level, never inside
`.config/`. A `.config/ryl.toml` is a true drop-in for a root `ryl.toml`: its
`[files]`/`exclude` globs and relative `exclude-from-file` paths resolve against
the project root (the directory containing `.config/`), not `.config/` itself.

If you already have a yamllint configuration, use the built-in converter:

```bash
ryl --migrate-configs --migrate-write
```

See [Migrating from yamllint](migrating-from-yamllint.md) for details.

Earlier releases put `[rules]`, `[fix]`, `per-file-ignores` and
`per-line-ignores` at the top level. ryl still reads them, but warns once per
key, naming its `[lint]` replacement; when both spellings are set, the `[lint]`
one wins. The same `ryl --migrate-configs --migrate-write` rewrites such a
`.ryl.toml`/`ryl.toml` in place, dropping its comments (add
`--migrate-rename-old .bak` to keep the original as `.ryl.toml.bak`), and
`ryl --migrate-user-config --migrate-write` does the same for the user-global
config. For `pyproject.toml` it only prints the keys to move, since rewriting
would drop the rest of the file's comments and layout.

## Lint alongside `ryl format`

If you run `ryl format`, let it own layout and lint only what it never touches. This
starter config enables the rules that catch YAML that loads to something other than
what the author meant, and conflicts with no `[format]` setting:

<!-- ryl-config-check: format-clean -->
```toml
[lint.rules]
anchors = "enable"
key-duplicates = "enable"
truthy = "enable"

[lint.per-file-ignores]
".github/workflows/*" = ["truthy"]
```

Gate CI on both:

```bash
ryl format --check .
ryl check .
```

The `truthy` ignore keeps GitHub Actions' `on:` key from being reported (see
[`truthy`](../rules/truthy.md)). Add any other rule from the
[Rules reference](../rules.md) as you need it; the [Formatter](../formatter.md#conflicting-lint-rules)
page lists the layout rules and the options that conflict with `ryl format`. If you do
not run `ryl format`, start from a [preset](../config-presets.md) instead, which keeps
the layout rules.

## Configure across projects (user-global)

When no project config is found, ryl falls back to a user-global config so you
can set personal defaults once. It reads its own TOML config first &mdash;
`<config-dir>/ryl/.ryl.toml` (or `ryl.toml`), following the ruff/Biome
convention where `<config-dir>` is `$XDG_CONFIG_HOME` if set, else the
platform-native config dir (`~/.config/ryl` on Linux, `~/Library/Application
Support/ryl` on macOS, `%APPDATA%\ryl` on Windows) &mdash; then falls back to
yamllint's `<config-dir>/yamllint/config`, which is deprecated and warns. A project config,
`-c`/`-d`, or `YAMLLINT_CONFIG_FILE` all take precedence over the user-global
config. `YAMLLINT_CONFIG_FILE` is deprecated and accepts only a yamllint YAML
config (pointing it at a `.toml` errors); use `-c` for ryl-native TOML.

If you have a yamllint user-global config, `ryl --migrate-user-config
--migrate-write` converts it to the ryl-native `ryl.toml` (see [Migrating from
yamllint](migrating-from-yamllint.md)).

## Configuration precedence

ryl resolves the configuration governing **each file** it lints, trying these
sources in order and stopping at the first hit. `-d`/`-c` and
`YAMLLINT_CONFIG_FILE` pin a single config for the whole run; otherwise project
discovery runs per file, so a monorepo can hold many `.ryl.toml` files, each
governing its own subtree. The winning config must enable at least one rule, or
`--enable` must name one, or ryl exits `2`:

```mermaid
flowchart TD
    Start([resolve config]) --> D{"-d / --config-data?"}
    D -->|yes| UseInline["use inline TOML,<br/>or YAML (deprecated)"] --> Done([config resolved])
    D -->|no| C{"-c / --config-file?"}
    C -->|yes| UseFile["load file: TOML, or YAML<br/>(deprecated) by extension"] --> Done
    C -->|no| P{"project config?<br/>walk up from inputs to HOME"}
    P -->|"TOML up-tree"| UseProjToml["nearest TOML:<br/>.ryl.toml &gt; ryl.toml<br/>&gt; .config/.ryl.toml &gt; .config/ryl.toml<br/>&gt; pyproject.toml [tool.ryl]"] --> Done
    P -->|"else .yamllint up-tree"| UseProjYaml["nearest .yamllint /<br/>.yamllint.yaml / .yamllint.yml<br/>(deprecated)"] --> Done
    P -->|none| E{"YAMLLINT_CONFIG_FILE set?"}
    E -->|"points at .toml"| Err1["error: use -c / project discovery<br/>for ryl TOML (exit 2)"]
    E -->|"YAML and exists"| UseEnv["load as yamllint YAML<br/>(deprecated)"] --> Done
    E -->|"missing or unset"| G{"user-global config?"}
    G -->|"ryl TOML"| UseRyl["config-dir/ryl/.ryl.toml &gt; ryl.toml"] --> Done
    G -->|"else yamllint YAML"| UseYl["config-dir/yamllint/config<br/>(deprecated)"] --> Done
    G -->|none| N{"--enable?"}
    N -->|yes| UseEmpty["empty config"] --> Done
    N -->|no| Err2["error: no configuration found (exit 2)"]
    Done --> R{"any rule enabled,<br/>after --enable?"}
    R -->|yes| OK([lint])
    R -->|no| Err3["error: no rules enabled (exit 2)"]
    classDef default stroke-width:3px;
    linkStyle default stroke-width:3px;
```
