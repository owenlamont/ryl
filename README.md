# ryl

ryl - the Rust Yaml Linter is intended to ultimately be a drop in replacement for
[yamllint](https://github.com/adrienverge/yamllint). It is usable today, but
parity and edge-case behavior are still maturing.

Compatibility note:

- `ryl` aims to match `yamllint` behavior and includes many parity tests.
- `ryl` uses the `saphyr` parser stack, while `yamllint` uses the `PyYAML` parser stack.
- `saphyr` and `PyYAML` do not always agree on which files are valid YAML.

## Quick Start

```bash
# Using uv (Python)
uvx ryl .

# Using npx (Node.js)
npx @owenlamont/ryl .
```

For `prek` / `pre-commit` integration, see
[ryl-pre-commit](https://github.com/owenlamont/ryl-pre-commit).

## Installation

### uv

```bash
uv tool install ryl
```

### NPM

```bash
npm install -g @owenlamont/ryl
```

### pip

```bash
pip install ryl
```

### Cargo

```bash
cargo install ryl
```

## Usage

ryl accepts one or more paths: files and/or directories.

Basic:

```text
ryl <PATH_OR_FILE> [PATH_OR_FILE...]
```

Behavior:

- Files: parsed as YAML even if the extension is not `.yml`/`.yaml`.
- Directories: recursively lints `.yml` and `.yaml` files.
  - Respects `.gitignore`, global git ignores, and git excludes.
  - Does not follow symlinks.

Exit codes:

- `0` when all parsed files are valid (or no files found).
- `1` when any invalid YAML is found.
- `2` for CLI usage errors (for example, no paths provided).

Examples:

```text
# Single file
ryl myfile.yml

# Multiple inputs (mix files and directories)
ryl config/ another.yml

# Multiple directories
ryl dir1 dir2

# Explicit non-YAML extension (parsed as YAML)
ryl notes.txt
```

Help and version:

```text
Fast YAML linter written in Rust

Usage: ryl [OPTIONS] [PATH_OR_FILE]...

Arguments:
  [PATH_OR_FILE]...  One or more paths: files and/or directories

Options:
  -c, --config-file <FILE>           Path to configuration file (YAML or TOML)
  -d, --config-data <YAML>           Inline configuration data (yaml)
  -f, --format <FORMAT>              Output format (auto, standard, colored, github,
                                     parsable) [default: auto]
                                     [possible values: auto, standard, colored,
                                     github, parsable]
      --print-toml-config-schema     Print the JSON Schema for ryl TOML config
                                     and exit
      --print-yaml-config-schema     Print the JSON Schema for yamllint-compatible
                                     YAML config and exit
      --fix                          Apply safe fixes in place before reporting
                                     remaining diagnostics
      --migrate-configs              Convert discovered legacy YAML config files
                                     into .ryl.toml files
      --list-files                   List files that would be linted (reserved)
  -s, --strict                       Strict mode (reserved)
      --no-warnings                  Suppress warnings (reserved)
      --migrate-root <DIR>           Root path to search for legacy YAML config
                                     files (default: .)
      --migrate-write                Write migrated .ryl.toml files (otherwise
                                     preview only)
      --migrate-stdout               Print generated TOML to stdout during migration
      --migrate-rename-old <SUFFIX>  Rename source YAML configs by appending
                                     this suffix after migration
      --migrate-delete-old           Delete source YAML configs after migration
  -h, --help                         Print help
  -V, --version                      Print version
```

## Performance benchmarking

This repo includes a standalone benchmark script that compares PyPI `ryl` and
`yamllint` using synthetic YAML corpora and `hyperfine`.

Prerequisites:

- `uv`
- `hyperfine`

Run a quick sample:

```text
uv run scripts/benchmark_perf_vs_yamllint.py --file-counts 25,100 --file-sizes-kib 1,8 --runs 5 --warmup 1
```

Run a fuller matrix (explicit lists):

```text
uv run scripts/benchmark_perf_vs_yamllint.py --file-counts 25,100,400,1000 --file-sizes-kib 1,8,32,128 --runs 10 --warmup 2
```

Run a fuller matrix (ranges with increments):

```text
uv run scripts/benchmark_perf_vs_yamllint.py --file-count-start 100 --file-count-end 1000 --file-count-step 100 --file-size-start-kib 4 --file-size-end-kib 64 --file-size-step-kib 4 --runs 10 --warmup 2
```

The script uses Typer; use `--help` for all options.

Artifacts are written under `manual_outputs/benchmarks/<UTC_TIMESTAMP>/`:

- `benchmark.png` and `benchmark.svg`: side-by-side facet plot with shared Y axis.
- `summary.csv`: aggregated timing table.
- `meta.json`: tool versions and run parameters.
- `hyperfine-json/`: raw results from `hyperfine`.

Example benchmark figure (5x5 matrix, 5 runs per point):

![Benchmark: ryl vs yamllint scaling (5x5 matrix, 5 runs per point)](https://raw.githubusercontent.com/owenlamont/ryl/v0.3.4/img/benchmark-5x5-5runs.svg)

## Configuration

- Flags:
  - `-c, --config-file <FILE>`: path to a YAML or TOML config file.
  - `-d, --config-data <YAML>`: inline YAML config (highest precedence).
  - `--print-toml-config-schema`: print the JSON Schema for `.ryl.toml` / `ryl.toml` / `[tool.ryl]`.
  - `--print-yaml-config-schema`: print the JSON Schema for yamllint-compatible
    YAML config files such as `.yamllint` / `.yamllint.yml` / `.yamllint.yaml`.
  - `--fix`: apply safe fixes in place before reporting remaining diagnostics.
  - `--list-files`: print files that would be linted after applying ignores and exit.
  - `--migrate-configs`: discover legacy YAML configs and plan TOML migration.
  - `--migrate-root <DIR>`: root to search for legacy YAML configs (default `.`).
  - `--migrate-write`: write migrated `.ryl.toml` files (without this it is preview-only).
  - `--migrate-stdout`: print generated TOML in migration mode.
  - `--migrate-rename-old <SUFFIX>`: rename discovered legacy YAML config files after writing.
  - `--migrate-delete-old`: delete discovered legacy YAML config files after writing.
  - `-f, --format`, `-s, --strict`, `--no-warnings`: reserved for compatibility.
- Discovery precedence:
  inline `--config-data` > `--config-file` > env `YAMLLINT_CONFIG_FILE`
  (global) > nearest project config up the tree:
  TOML (`.ryl.toml`, `ryl.toml`, `pyproject.toml` with `[tool.ryl]`) then
  YAML fallback (`.yamllint`, `.yamllint.yml`, `.yamllint.yaml`)
  > user-global (`$XDG_CONFIG_HOME/yamllint/config` or
  `~/.config/yamllint/config`) > built-in defaults.
- **Rules Documentation**: see [docs/rules.md](https://github.com/owenlamont/ryl/blob/main/docs/rules.md)
  for a full list of supported rules and their fixable status.
- TOML and YAML are not merged during discovery. If a TOML project config is
  found, YAML project config discovery is skipped (and `ryl` prints a warning).
- Native fix policy is TOML-only. YAML config remains yamllint-compatible and
  does not support `fix` settings.
- Per-file behavior: unless a global config is set via `--config-data`,
  `--config-file`, or `YAMLLINT_CONFIG_FILE`, each file discovers its nearest
  project config. Ignores apply to directory scans and explicit files (parity).
- Presets and extends: supports yamllint’s built-in `default`, `relaxed`, and
  `empty` via `extends`. Rule maps are deep-merged; scalars/sequences overwrite.
- TOML preset examples: see
  [docs/config-presets.md](https://github.com/owenlamont/ryl/blob/main/docs/config-presets.md)
  for `default`/`relaxed` equivalents.
- Canonical schema artifacts are checked into this repo as:
  - `ryl.toml.schema.json` for `.ryl.toml` / `ryl.toml` / `[tool.ryl]`
  - `ryl.yaml.schema.json` for yamllint-compatible YAML config files such as
    `.yamllint` / `.yamllint.yml` / `.yamllint.yaml`
- SchemaStore sync only targets the native TOML config and publishes a draft-07
  projection for `ryl.toml` / `.ryl.toml`. SchemaStore cannot target the
  `[tool.ryl]` table inside `pyproject.toml`, so that remains covered by the
  broader `pyproject.toml` schema association.
- Release-time SchemaStore sync updates `owenlamont/schemastore:ryl-schema-update`
  after a release succeeds and prints the manual upstream PR details in the
  workflow summary.
- Regenerate them with:
  - `cargo run --quiet --bin ryl -- --print-toml-config-schema > ryl.toml.schema.json`
  - `cargo run --quiet --bin ryl -- --print-yaml-config-schema > ryl.yaml.schema.json`
- Print the SchemaStore projection locally with:
  - `uv run scripts/print_ryl_schemastore_schema.py > /tmp/ryl.schemastore.json`

Example TOML config (`.ryl.toml`):

```toml
yaml-files = ["*.yaml", "*.yml"]
ignore = ["vendor/**", "generated/**"]
locale = "en_US.UTF-8"

[rules]
document-start = "disable"

[rules.line-length]
max = 120

[rules.truthy]
allowed-values = ["true", "false"]

[fix]
fixable = ["ALL"]
unfixable = []
```

For a fully expanded TOML example that names every built-in rule explicitly, see
[`/.ryl.toml.example`](https://github.com/owenlamont/ryl/blob/main/.ryl.toml.example).

Migration example:

```text
# Preview migration actions
ryl --migrate-configs --migrate-root .

# Write .ryl.toml files and keep old files with a suffix
ryl --migrate-configs --migrate-root . --migrate-write --migrate-rename-old .bak
```

## Acknowledgements

This project exists thanks to the tooling and ecosystems around YAML linting and
developer automation, especially:

- [yamllint](https://github.com/adrienverge/yamllint) - for giving me the shoulders to
  stand on and the source of many of the automated tests that ryl uses now to check for
  behaviour parity. Copying the behaviour of an existing tool is infinitely easier than
  building one from scratch - there'd be no ryl without yamllint.
- [ruff](https://github.com/astral-sh/ruff) - for showing the power of Rust tooling for
  Python development and inspiring the config and API for ryl.
- [rumdl](https://github.com/rvben/rumdl) - for giving me another template to follow for
  Rust tooling and showing me almost the only dev tool I was still using after this that
  wasn't written in Rust was yamllint (which inspired me to tackle this project)
- [saphyr](https://github.com/saphyr-rs/saphyr) - ryl is built on saphyr and saphyr's
  developers were very patient in showing some of the nuance and complexity of parsing
  YAML which I was embarrassingly ignorant of when start ryl.
- [esbuild](https://github.com/evanw/esbuild) and
  [biome](https://github.com/biomejs/biome) - for providing the "binary wrapper"
  blueprint for distributing high-performance native tools via NPM.
