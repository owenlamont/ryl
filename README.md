# ryl

ryl is a fast YAML linter and formatter written in Rust. It checks YAML style,
applies safe lint fixes, and formats files with configurable layout. It is usable
today, but edge-case behaviour is still maturing.

Full documentation lives at <https://ryl-docs.pages.dev/>.

ryl targets YAML 1.2. See [YAML version compatibility](https://ryl-docs.pages.dev/yaml-version/)
for how version directives and implicit types affect linting and formatting.

Coming from yamllint? See the [migration guide](https://ryl-docs.pages.dev/getting-started/migrating-from-yamllint/).

## Quick start

ryl enables no rules by default, so it needs a configuration or `--enable` that
turns rules on. `--enable ALL` runs every rule for a one-off run, and
`--enable <RULES>` runs just the named rules; for a project,
drop a `.ryl.toml` at the root (see
[Configuration](https://ryl-docs.pages.dev/getting-started/quickstart/)).

`ryl check` is the lint subcommand and the recommended form; `ryl format` is the
formatter. Bare `ryl <paths>` still lints identically but is deprecated: it
prints a warning to stderr (silenced by `--no-warnings`) and a later release will
remove it, so prefer `ryl check`.

```bash
# Using uv (Python)
uvx ryl check --enable ALL .

# Using npx (Node.js)
npx @owenlamont/ryl check --enable ALL .

# Format in place, or check formatting in CI
uvx ryl format .
uvx ryl format --check .
```

For `prek` / `pre-commit` integration, see
[ryl-pre-commit](https://github.com/owenlamont/ryl-pre-commit).

## Installation

```bash
uv tool install ryl                 # uv
npm install -g @owenlamont/ryl      # npm
pip install ryl                     # pip
cargo install ryl                   # cargo
pixi global install ryl             # conda-forge
winget install owenlamont.ryl       # winget (Windows)
```

Building from source (`cargo install`, or a `pip` sdist) needs Rust 1.99.0 or
newer. The prebuilt wheels, npm, conda-forge and winget packages do not.

## Status and scope

- `ryl format` formats YAML in place without enabling lint rules. Its `--check`
  and `--diff` modes preview changes without writing; `[format]` in TOML controls
  the style. It also formats embedded YAML in Markdown with `--markdown`.
- ryl includes 27 rules, including `tags`, `merge-keys`,
  `block-scalar-chomping`, and `unicode-line-breaks`. The rule reference
  and per-rule pages are at <https://ryl-docs.pages.dev/rules/>.
- Auto-fixing (`--fix`) is supported for `braces`, `brackets`, `colons`,
  `commas`, `comments`, `comments-indentation`, `document-end`,
  `document-start`, `empty-lines`, `hyphens`, `key-ordering`,
  `new-line-at-end-of-file`, `new-lines`, `quoted-strings`, `trailing-spaces`,
  and `truthy` (case only, e.g. `True` to `true`). The set of rules that may
  apply fixes is configurable via the TOML `lint.fixable`/`lint.unfixable`
  keys.
- `--diff` previews those safe fixes as a unified diff on stdout instead
  of writing them (modelled on `ruff check --diff`); it is mutually
  exclusive with `--fix`, works with stdin, and exits `1` iff some file
  would change.
- TOML is the recommended configuration format and supports
  the `lint.fixable`/`lint.unfixable`
  fix policy, `[lint.per-file-ignores]`, and rule options such as
  `allow-double-quotes-for-escaping`.
- TOML assigns each file a source kind via the `[files]` table
  (`yaml = [...]`, `markdown = [...]`). The legacy `yaml-files` key is YAML-only;
  in TOML use `[files].yaml`.
- YAML embedded in Markdown (front matter and fenced `yaml`/`yml` blocks)
  can be linted by listing globs under `[files].markdown`; diagnostics map back
  to the Markdown file. It is off by default; `--fix` writes safe fixes back into
  the embedded blocks and `--diff` previews them at the host-file level. See
  <https://ryl-docs.pages.dev/markdown/>.
- YAML configuration (`.yamllint`, `.yamllint.yml`,
  `.yamllint.yaml`, including the `default`, `relaxed`, and `empty` presets via
  `extends`) still works but is deprecated and warns; convert it with
  `ryl --migrate-configs --migrate-write`.
- `--list-files` prints the files ryl would lint (after ignores and
  config discovery) and exits, without running rules. `--no-warnings`
  suppresses warning-level diagnostics in the output. `--strict` turns a
  warning-only run into exit code `2`. `--enable <RULES>` runs exactly the
  listed rules (comma-separated IDs, or `ALL`) in place of the config's
  selection, and needs no config file.
- Pass `-` as the input to read YAML from stdin (ruff convention). Add
  `--stdin-filename <PATH>` so diagnostics, project-config discovery,
  and path-based filtering (`yaml-files`, per-file-ignores, per-rule
  `ignore`) use that filename. Without it, diagnostics are labelled
  `<stdin>`, config is anchored at the current working directory, and
  all path-based filtering is skipped so every enabled rule runs.
  `-` cannot be combined with other inputs; `--fix` cannot read from
  stdin, but `--diff` can.
- `ryl server` runs ryl as a Language Server (LSP) over stdio, giving
  editors inline diagnostics, a `source.fixAll.ryl` code action, and
  formatting via `ryl format`. It complements Red Hat's
  `yaml-language-server` rather than replacing it, and is built in by
  default (a minimal binary without it builds with
  `cargo install ryl --no-default-features`). See
  <https://ryl-docs.pages.dev/editor-integration/>.
- Run `ryl --help` for the authoritative CLI reference.

For installation walkthroughs, configuration presets, and per-rule
documentation, see the docs site.

## Performance

The figure times lint and in-place format on the same deliberately
unformatted YAML in a 5×5 file-count × file-size matrix, with five
hyperfine runs per point. Formatting starts from a fresh copy for every run.
It predates the property-test corpus now used by the benchmark script.

![YAML lint and format benchmark](https://raw.githubusercontent.com/owenlamont/ryl/main/img/benchmark-5x5-5runs.svg)

| Tool | Version | Configuration |
| --- | --- | --- |
| ryl | 0.25.1 | Lint: `extends: relaxed`; format: `[format]` defaults |
| yamllint | 1.38.0 | `extends: relaxed` |
| google/yamlfmt | 0.21.0 | Basic formatter, LF line endings |
| Prettier | 3.9.10 | Defaults; no config, EditorConfig, or ignore rules |
| dprint | 0.63.0 | `pretty_yaml` 0.6.0 defaults; incremental mode off |

Tool pins live in `scripts/benchmark_tool_versions.toml`; the benchmark's
`--install-pinned-tools` option installs them. Run
`uv run scripts/benchmark_yaml_tools.py --help` for other options. Each run
writes the commands, binary hashes, versions, configuration, corpus hash,
seed, strategy, rejection rate, raw samples, and summary to
`manual_outputs/benchmarks/`. Generated YAML is temporary and untracked.
[Biome](https://biomejs.dev/introduction/language-support/) 2.5.15 is absent
because Biome does not process YAML files.

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
  template to follow for Rust tooling.
- [saphyr](https://github.com/saphyr-rs/saphyr) - ryl's vendored YAML DOM
  is derived from saphyr, and saphyr's developers were very patient in
  showing some of the nuance and complexity of parsing YAML which I was
  embarrassingly ignorant of when starting ryl.
- [granit-parser](https://github.com/bourumir-wyngs/granit-parser) - a
  `saphyr-parser` fork with comment and style metadata that ryl uses for
  event-stream parsing.
- [esbuild](https://github.com/evanw/esbuild) and
  [biome](https://github.com/biomejs/biome) - for providing the "binary
  wrapper" blueprint for distributing high-performance native tools via
  NPM.
