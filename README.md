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
uvx ryl .
```

For `prek` / `pre-commit` integration, see
[ryl-pre-commit](https://github.com/owenlamont/ryl-pre-commit).

## Installation

### uv

```bash
uv tool install ryl
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

- `ryl -h` or `ryl --help` shows auto-generated help.
- `ryl -V` or `ryl --version` prints the version.

The CLI is built with `clap`, which auto-generates `--help` and `--version`.

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
  - `-c, --config-file <FILE>`: path to a YAML config file.
  - `-d, --config-data <YAML>`: inline YAML config (highest precedence).
  - `--list-files`: print files that would be linted after applying ignores and exit.
  - `-f, --format`, `-s, --strict`, `--no-warnings`: reserved for compatibility.
- Discovery precedence:
  inline `--config-data` > `--config-file` > env `YAMLLINT_CONFIG_FILE`
  (global) > nearest project config up the tree (`.yamllint`, `.yamllint.yml`,
  `.yamllint.yaml`) > user-global (`$XDG_CONFIG_HOME/yamllint/config` or
  `~/.config/yamllint/config`) > built-in defaults.
- Per-file behavior: unless a global config is set via `--config-data`,
  `--config-file`, or `YAMLLINT_CONFIG_FILE`, each file discovers its nearest
  project config. Ignores apply to directory scans and explicit files (parity).
- Presets and extends: supports yamllint’s built-in `default`, `relaxed`, and
  `empty` via `extends`. Rule maps are deep-merged; scalars/sequences overwrite.

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
