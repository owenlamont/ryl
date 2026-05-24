# ryl

ryl - the Rust YAML linter - is intended to ultimately be a drop-in
replacement for [yamllint](https://github.com/adrienverge/yamllint). It is
usable today, but parity and edge-case behaviour are still maturing.

Full documentation lives at <https://ryl-docs.pages.dev/>.

## Compatibility note

- `ryl` aims to match `yamllint` behaviour and includes many parity tests.
- `ryl` uses the `saphyr` parser stack, while `yamllint` uses the `PyYAML`
  parser stack.
- `saphyr` and `PyYAML` do not always agree on which files are valid YAML.
- **`ryl` targets YAML 1.2 strictly.** `yamllint` defaults to YAML 1.1
  semantics, so bareword booleans like `yes` / `no` / `on` / `off` (and
  case variants) are plain strings in `ryl` and booleans in `yamllint`.
  Leading-zero integers like `0755` are decimal in `ryl` and octal in
  `yamllint`. The same 1.2 semantics also apply when `ryl` parses
  `.yamllint` configuration files inherited from `yamllint`. See the
  [YAML version compatibility](https://ryl-docs.pages.dev/yaml-version/)
  page for the practical implications.

## Quick start

```bash
# Using uv (Python)
uvx ryl .

# Using npx (Node.js)
npx @owenlamont/ryl .
```

For `prek` / `pre-commit` integration, see
[ryl-pre-commit](https://github.com/owenlamont/ryl-pre-commit).

## Installation

```bash
uv tool install ryl                 # uv
npm install -g @owenlamont/ryl      # npm
pip install ryl                     # pip
cargo install ryl                   # cargo
```

## Status and scope

- All 23 yamllint rules are implemented. The current rule reference and
  per-rule pages are at
  <https://ryl-docs.pages.dev/rules/>.
- Auto-fixing (`--fix`) is supported for `braces`, `brackets`, `commas`,
  `comments`, `comments-indentation`, `new-line-at-end-of-file`,
  `new-lines`, and `quoted-strings`. The set of rules that may apply
  fixes is configurable via the TOML `[fix]` table.
- TOML is the recommended configuration format and supports ryl-only
  features that have no upstream equivalent: the `[fix]` table,
  `[per-file-ignores]`, and rule options such as
  `allow-double-quotes-for-escaping`.
- yamllint-style YAML configuration is also accepted (`.yamllint`,
  `.yamllint.yml`, `.yamllint.yaml`) for drop-in compatibility, including
  the built-in `default`, `relaxed`, and `empty` presets via `extends`.
  An existing yamllint configuration can be converted with
  `ryl --migrate-configs --migrate-write`.
- `--list-files` prints the files ryl would lint (after ignores and
  config discovery) and exits, without running rules. `--no-warnings`
  suppresses warning-level diagnostics in the output. `--strict` turns a
  warning-only run into exit code `2`.
- Pass `-` as the input to read YAML from stdin (ruff convention). Add
  `--stdin-filename <PATH>` so diagnostics, project-config discovery,
  and path-based filtering (`yaml-files`, per-file-ignores, per-rule
  `ignore`) use that filename. Without it, diagnostics are labelled
  `<stdin>`, config is anchored at the current working directory, and
  all path-based filtering is skipped so every enabled rule runs.
  `-` cannot be combined with other inputs or with `--fix`.
- Run `ryl --help` for the authoritative CLI reference.

For installation walkthroughs, configuration presets, and per-rule
documentation, see the docs site.

## Performance

`ryl` is significantly faster than `yamllint` on large trees. The figure
below is from a 5x5 file-count × file-size matrix with 5 runs per point:

![Benchmark: ryl vs yamllint scaling (5x5 matrix, 5 runs per point)](https://raw.githubusercontent.com/owenlamont/ryl/main/img/benchmark-5x5-5runs.svg)

The benchmark script is in `scripts/benchmark_perf_vs_yamllint.py`; run
it with `--help` for the full option set.

## Acknowledgements

This project exists thanks to the tooling and ecosystems around YAML
linting and developer automation, especially:

- [yamllint](https://github.com/adrienverge/yamllint) - for giving me the
  shoulders to stand on and the source of many of the automated tests
  that ryl uses now to check for behaviour parity. Copying the behaviour
  of an existing tool is infinitely easier than building one from
  scratch - there'd be no ryl without yamllint.
- [ruff](https://github.com/astral-sh/ruff) - for showing the power of
  Rust tooling for Python development and inspiring the config and API
  for ryl.
- [rumdl](https://github.com/rvben/rumdl) - for giving me another
  template to follow for Rust tooling and showing me almost the only dev
  tool I was still using after this that wasn't written in Rust was
  yamllint (which inspired me to tackle this project).
- [saphyr](https://github.com/saphyr-rs/saphyr) - ryl is built on saphyr
  and saphyr's developers were very patient in showing some of the
  nuance and complexity of parsing YAML which I was embarrassingly
  ignorant of when starting ryl.
- [esbuild](https://github.com/evanw/esbuild) and
  [biome](https://github.com/biomejs/biome) - for providing the "binary
  wrapper" blueprint for distributing high-performance native tools via
  NPM.
